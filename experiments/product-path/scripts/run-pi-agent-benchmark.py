#!/usr/bin/env python3
"""Run real Pi RPC sessions against the Raccord semantic tools."""

from __future__ import annotations

import argparse
import json
import os
import selectors
import subprocess
import time
from pathlib import Path
from typing import cast

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "fixtures" / "agent-benchmark.json"
EXTENSION = ROOT / "pi" / "raccord-tools.ts"
TOOL_MAP = {
    "raccord_find": "find",
    "raccord_inspect": "inspect",
    "raccord_plan_edit": "plan_edit",
    "raccord_commit_edit": "commit_edit",
    "raccord_verify": "verify",
}

SYSTEM_PROMPT = """You are an Agent editing a Raccord media timeline.
Use only raccord_* tools. Never use shell commands, absolute frame positions, track indexes, FFmpeg expressions, or raw IR.
For every task: discover stable clip IDs with raccord_find, inspect the current revision with raccord_inspect, plan semantic edits with raccord_plan_edit, commit using the returned plan token, and finish with raccord_verify.
If a tool returns a structured error, reason from it and recover. Use these exact edit fields: ripple_delete uses clip_id; replace_source uses clip_id and source; trim uses clip_id and duration_frames; insert_after uses after and clip; move_after uses clip_id and after. For metadata edits, set_audio_gain uses clip_id and gain_db_milli; add_marker uses id, clip_id, and label; add_subtitle uses id, clip_id, and text. add_transition uses id, from_clip_id, to_clip_id, kind=crossfade, and duration_frames. Do not substitute id for clip_id. Do not use after for metadata edits. Do not claim success until raccord_verify passes. Keep your final response to one short sentence after verification."""


def require(condition: bool, detail: str) -> None:
    if not condition:
        raise RuntimeError(detail)


def read_event(stream, timeout: float) -> dict:
    selector = selectors.DefaultSelector()
    try:
        selector.register(stream, selectors.EVENT_READ)
        if not selector.select(timeout):
            raise RuntimeError(f"Pi RPC timed out after {timeout:.1f}s")
        line = stream.readline()
    finally:
        selector.close()
    if not line:
        raise RuntimeError("Pi RPC process ended unexpectedly")
    try:
        event = json.loads(line)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"Pi emitted invalid JSON: {line}") from error
    require(isinstance(event, dict), "Pi RPC event must be an object")
    return cast(dict, event)


def send(proc: subprocess.Popen, message: dict) -> None:
    stdin = proc.stdin
    if stdin is None:
        raise RuntimeError("Pi RPC stdin is unavailable")
    stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
    stdin.flush()


def load_tasks(path: Path) -> list[dict]:
    try:
        with path.open(encoding="utf-8") as stream:
            tasks = json.load(stream)
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError(f"cannot load Pi benchmark fixture: {path}") from error
    require(isinstance(tasks, list), "Pi benchmark fixture must be a list")
    return cast(list[dict], tasks)


def tool_requests(events: list[dict]) -> list[dict]:
    requests = []
    for event in events:
        if event.get("type") != "message_end":
            continue
        message = event.get("message")
        if not isinstance(message, dict) or message.get("role") != "assistant":
            continue
        content = message.get("content", [])
        if not isinstance(content, list):
            continue
        for item in content:
            if not isinstance(item, dict) or item.get("type") != "toolCall":
                continue
            tool_name = item.get("name")
            if tool_name not in TOOL_MAP:
                continue
            arguments = item.get("arguments", {})
            require(
                isinstance(arguments, dict),
                f"Pi tool arguments must be an object: {item}",
            )
            request = {"tool": TOOL_MAP[tool_name]}
            if arguments:
                request["args"] = arguments
            requests.append(request)
    return requests


def run_task(
    task: dict, model: str, timeout: float, timeline: Path | None = None
) -> dict:
    command = [
        "pi",
        "--mode",
        "rpc",
        "--no-session",
        "--no-extensions",
        "--no-skills",
        "--no-context-files",
        "--no-builtin-tools",
        "--extension",
        str(EXTENSION),
        "--model",
        model,
        "--thinking",
        "minimal",
        "--system-prompt",
        SYSTEM_PROMPT,
    ]
    environment = os.environ.copy()
    if timeline:
        environment["RACCORD_TIMELINE"] = str(timeline.resolve())
    proc = subprocess.Popen(
        command,
        cwd=ROOT.parent.parent,
        env=environment,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    events = []
    try:
        send(proc, {"id": "prompt-1", "type": "prompt", "message": task["prompt"]})
        deadline = time.monotonic() + timeout
        settled = False
        while not settled:
            remaining = deadline - time.monotonic()
            require(remaining > 0, f"{task['id']} exceeded Pi timeout")
            event = read_event(proc.stdout, remaining)
            events.append(event)
            settled = event.get("type") == "agent_settled"

        send(proc, {"id": "stats-1", "type": "get_session_stats"})
        stats = None
        while stats is None:
            remaining = deadline - time.monotonic()
            require(remaining > 0, f"{task['id']} exceeded Pi stats timeout")
            event = read_event(proc.stdout, remaining)
            events.append(event)
            if event.get("type") == "response" and event.get("id") == "stats-1":
                require(
                    event.get("success", False), f"Pi stats request failed: {event}"
                )
                stats = event.get("data", {})
        requests = tool_requests(events)
        usage = stats.get("tokens", {}) if isinstance(stats, dict) else {}
        transcript = {"id": task["id"], "requests": requests}
        if isinstance(usage, dict) and usage.get("total") is not None:
            transcript["usage"] = {
                "input_tokens": usage.get("input", 0),
                "output_tokens": usage.get("output", 0),
                "cache_read_tokens": usage.get("cacheRead", 0),
                "cache_write_tokens": usage.get("cacheWrite", 0),
                "total_tokens": usage.get("total", 0),
            }
        return transcript
    finally:
        if proc.stdin:
            proc.stdin.close()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        if proc.returncode not in (0, None):
            detail = proc.stderr.read() if proc.stderr else ""
            raise RuntimeError(f"Pi failed for {task['id']}: {detail}")


def write_json(path: Path, value: object, description: str) -> None:
    try:
        path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    except OSError as error:
        raise RuntimeError(f"cannot write Pi {description}: {path}") from error


def run_benchmark(
    fixture: Path,
    transcript: Path,
    report: Path | None,
    timeline: Path | None = None,
) -> dict | None:
    command = [
        "python3",
        str(ROOT / "scripts" / "run-agent-benchmark.py"),
        "--fixture",
        str(fixture),
        "--transcript",
        str(transcript),
    ]
    if timeline:
        command.extend(("--timeline", str(timeline)))
    if report:
        command.extend(("--report", str(report)))
    completed = subprocess.run(command, check=False, capture_output=True, text=True)
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"Pi benchmark validation failed: {detail}")
    print(completed.stdout, end="")
    if report is None:
        return None
    try:
        return cast(dict, json.loads(report.read_text(encoding="utf-8")))
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError(f"cannot read Pi benchmark report: {report}") from error


def run_path(path: Path, run_number: int, total_runs: int) -> Path:
    if total_runs == 1:
        return path
    return path.with_name(f"{path.stem}.run-{run_number:02d}{path.suffix}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, default=FIXTURE)
    parser.add_argument("--timeline", type=Path, help="optional initial timeline JSON")
    parser.add_argument("--model", default="openai-codex/gpt-5.4-mini")
    parser.add_argument("--transcript-output", type=Path, required=True)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--timeout", type=float, default=180.0)
    parser.add_argument(
        "--runs", type=int, default=1, help="number of sequential Pi runs"
    )
    args = parser.parse_args()
    require(args.runs > 0, "--runs must be positive")

    tasks = load_tasks(args.fixture)
    run_reports = []
    failures = []
    for run_number in range(1, args.runs + 1):
        try:
            transcript_path = run_path(args.transcript_output, run_number, args.runs)
            transcript = [
                run_task(task, args.model, args.timeout, args.timeline)
                for task in tasks
            ]
            write_json(transcript_path, transcript, "transcript")
            report_path = (
                run_path(args.report, run_number, args.runs) if args.report else None
            )
            report = run_benchmark(
                args.fixture, transcript_path, report_path, args.timeline
            )
            run_reports.append(
                {"run": run_number, "status": "passed", "report": report}
            )
        except RuntimeError as error:
            detail = str(error)
            failures.append({"run": run_number, "error": detail})
            run_reports.append({"run": run_number, "status": "failed", "error": detail})
            print(f"Pi run {run_number} failed: {detail}")

    if args.runs > 1 and args.report:
        write_json(
            args.report,
            {"model": args.model, "runs": run_reports},
            "aggregate benchmark report",
        )
    if failures:
        raise RuntimeError(f"{len(failures)} of {args.runs} Pi runs failed")


if __name__ == "__main__":
    main()
