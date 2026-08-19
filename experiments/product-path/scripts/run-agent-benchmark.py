#!/usr/bin/env python3
"""Replay Agent-style reference transcripts and report protocol metrics."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
from pathlib import Path
from typing import cast

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "Cargo.toml"
FIXTURE = ROOT / "fixtures" / "agent-benchmark.json"


def require(condition: bool, detail: str) -> None:
    if not condition:
        raise RuntimeError(detail)


def require_equal(actual: object, expected: object, detail: str) -> None:
    if actual != expected:
        raise RuntimeError(f"{detail}: expected {expected!r}, got {actual!r}")


def invoke(
    requests: list[dict], timeline: Path | None = None
) -> tuple[list[dict], int, int]:
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
    if timeline:
        command.extend(("--", "--timeline", str(timeline.resolve())))
    payload = (
        "\n".join(json.dumps(request, separators=(",", ":")) for request in requests)
        + "\n"
    )
    environment = os.environ.copy()
    completed = subprocess.run(
        command,
        input=payload,
        text=True,
        check=True,
        capture_output=True,
        env=environment,
    )

    responses = []
    for line in completed.stdout.splitlines():
        if not line.strip():
            continue
        try:
            responses.append(json.loads(line))
        except json.JSONDecodeError as error:
            raise RuntimeError(f"invalid JSONL response: {line}") from error

    request_bytes = len(payload.encode("utf-8"))
    response_bytes = len(completed.stdout.encode("utf-8"))
    return responses, request_bytes, response_bytes


def validate_task(task: dict, timeline: Path | None = None) -> dict:
    responses, request_bytes, response_bytes = invoke(task["requests"], timeline)
    require_equal(len(responses), len(task["requests"]), f"{task['id']} response count")

    errors = [
        response["error"]["code"]
        for response in responses
        if not response.get("ok", False)
    ]
    acceptable_errors = task.get(
        "acceptable_error_sequences", [task["expected_errors"]]
    )
    require(errors in acceptable_errors, f"{task['id']} error sequence: {errors}")

    successful_commits = [
        response
        for response in responses
        if response.get("ok", False)
        and isinstance(response.get("result"), dict)
        and "version" in response["result"]
        and "placements" in response["result"]
    ]
    require(bool(successful_commits), f"{task['id']} has no successful commit result")
    final_commit = successful_commits[-1]["result"]
    acceptable_revisions = task.get("acceptable_revisions", [task["expected_revision"]])
    require(
        final_commit["version"] in acceptable_revisions,
        f"{task['id']} final revision: {final_commit['version']}",
    )
    require_equal(
        final_commit["placements"],
        task["expected_placements"],
        f"{task['id']} final placements",
    )
    for field in ("markers", "subtitles", "transitions"):
        expected = task.get(f"expected_{field}")
        if expected is not None:
            require_equal(
                final_commit.get(field),
                expected,
                f"{task['id']} final {field}",
            )

    final_response = responses[-1]
    require(
        final_response.get("ok", False), f"{task['id']} did not finish successfully"
    )
    require(final_response["result"]["pass"], f"{task['id']} verification failed")

    report = {
        "id": task["id"],
        "prompt_chars": len(task["prompt"]),
        "tool_calls": len(task["requests"]),
        "error_count": len(errors),
        "request_bytes": request_bytes,
        "response_bytes": response_bytes,
        "revision": task["expected_revision"],
    }
    usage = task.get("usage")
    if isinstance(usage, dict):
        for key in (
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
            "total_tokens",
        ):
            if key in usage:
                report[f"reported_{key}"] = usage[key]
    return report


def load_json(path: Path, description: str) -> object:
    try:
        with path.open(encoding="utf-8") as stream:
            return json.load(stream)
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError(f"cannot load {description}: {path}") from error


def load_tasks(fixture: Path) -> list[dict]:
    tasks = load_json(fixture, "benchmark fixture")
    require(
        isinstance(tasks, list), f"benchmark fixture must contain a list: {fixture}"
    )
    return cast(list[dict], tasks)


def merge_transcript(tasks: list[dict], transcript: Path | None) -> list[dict]:
    if transcript is None:
        return tasks

    entries = load_json(transcript, "Agent transcript")
    require(
        isinstance(entries, list), f"Agent transcript must contain a list: {transcript}"
    )
    entries = cast(list[dict], entries)
    by_id = {}
    for entry in entries:
        require(isinstance(entry, dict), "Agent transcript entries must be objects")
        task_id = entry.get("id")
        require(isinstance(task_id, str), "Agent transcript entry is missing string id")
        require(task_id not in by_id, f"duplicate Agent transcript id: {task_id}")
        require(
            isinstance(entry.get("requests"), list),
            f"{task_id} requests must be a list",
        )
        by_id[task_id] = entry

    task_by_id = {task["id"]: task for task in tasks}
    merged = []
    for task_id, entry in by_id.items():
        require(task_id in task_by_id, f"unknown Agent transcript id: {task_id}")
        task = dict(task_by_id[task_id])
        task["requests"] = entry["requests"]
        if "usage" in entry:
            task["usage"] = entry["usage"]
        merged.append(task)
    return merged


def summarize(reports: list[dict]) -> dict:
    summary: dict[str, int | str] = {
        "tasks": len(reports),
        "total_tool_calls": sum(report["tool_calls"] for report in reports),
        "total_errors": sum(report["error_count"] for report in reports),
        "total_request_bytes": sum(report["request_bytes"] for report in reports),
        "total_response_bytes": sum(report["response_bytes"] for report in reports),
    }
    usage_fields = {
        "total_input_tokens": "reported_input_tokens",
        "total_output_tokens": "reported_output_tokens",
        "total_cache_read_tokens": "reported_cache_read_tokens",
        "total_cache_write_tokens": "reported_cache_write_tokens",
        "total_tokens": "reported_total_tokens",
    }
    if any(field in report for report in reports for field in usage_fields.values()):
        for name, field in usage_fields.items():
            summary[name] = sum(report.get(field, 0) for report in reports)
        summary["note"] = "Token usage reported by the Agent provider."
    else:
        summary["note"] = (
            "LLM token usage is not measured until a real Agent transcript is supplied."
        )
    return summary


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--fixture",
        type=Path,
        default=FIXTURE,
        help="JSON benchmark fixture containing Agent prompts and JSONL requests",
    )
    parser.add_argument(
        "--transcript",
        type=Path,
        help="optional JSON transcript containing task ids and Agent-generated requests",
    )
    parser.add_argument(
        "--timeline",
        type=Path,
        help="optional initial timeline JSON passed to the semantic runtime",
    )
    parser.add_argument(
        "--report",
        type=Path,
        help="optional path for a machine-readable benchmark report",
    )
    args = parser.parse_args()

    tasks = merge_transcript(load_tasks(args.fixture), args.transcript)
    reports = [validate_task(task, args.timeline) for task in tasks]
    summary = summarize(reports)
    for report in reports:
        print(json.dumps(report, separators=(",", ":")))
    print(json.dumps(summary, separators=(",", ":")))

    if args.report:
        report = {"tasks": reports, "summary": summary}
        try:
            args.report.write_text(
                json.dumps(report, indent=2) + "\n",
                encoding="utf-8",
            )
        except OSError as error:
            raise RuntimeError(
                f"cannot write benchmark report: {args.report}"
            ) from error


if __name__ == "__main__":
    main()
