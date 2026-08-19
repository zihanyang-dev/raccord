#!/usr/bin/env python3
"""Route an interactive Agent process through the JSONL Raccord runtime."""

from __future__ import annotations

import argparse
import json
import selectors
import shlex
import subprocess
import sys
import time
from pathlib import Path
from typing import TextIO, cast

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "Cargo.toml"
FIXTURE = ROOT / "fixtures" / "agent-benchmark.json"


def require(condition: bool, detail: str) -> None:
    if not condition:
        raise RuntimeError(detail)


def load_tasks(path: Path) -> list[dict]:
    try:
        with path.open(encoding="utf-8") as stream:
            entries = json.load(stream)
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError(f"cannot load session fixture: {path}") from error
    require(isinstance(entries, list), "session fixture must contain a list")
    return cast(list[dict], entries)


def send(stream: TextIO, message: dict) -> None:
    stream.write(json.dumps(message, separators=(",", ":")) + "\n")
    stream.flush()


def read_message(stream: TextIO, timeout: float) -> dict:
    selector = selectors.DefaultSelector()
    try:
        selector.register(stream, selectors.EVENT_READ)
        if not selector.select(timeout):
            raise RuntimeError(f"Agent session timed out after {timeout:.1f}s")
        line = stream.readline()
    finally:
        selector.close()
    if not line:
        raise RuntimeError("Agent process ended without a session message")
    try:
        message = json.loads(line)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"Agent emitted invalid JSON: {line}") from error
    require(isinstance(message, dict), "Agent session message must be an object")
    return message


def runtime_command() -> list[str]:
    return [
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


def run_task(agent: subprocess.Popen, task: dict, timeout: float) -> dict:
    runtime = subprocess.Popen(
        runtime_command(),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    require(
        runtime.stdin is not None and runtime.stdout is not None,
        "runtime pipes unavailable",
    )
    require(
        agent.stdin is not None and agent.stdout is not None, "Agent pipes unavailable"
    )
    runtime_stdin = cast(TextIO, runtime.stdin)
    runtime_stdout = cast(TextIO, runtime.stdout)
    agent_stdin = cast(TextIO, agent.stdin)
    agent_stdout = cast(TextIO, agent.stdout)
    try:
        send(agent_stdin, {"type": "task", "id": task["id"], "prompt": task["prompt"]})
        requests = []
        responses = []
        usage = None
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            require(remaining > 0, f"{task['id']} exceeded session timeout")
            message = read_message(agent_stdout, remaining)
            message_type = message.get("type")
            if message_type == "request":
                request = message.get("request")
                require(isinstance(request, dict), "Agent request must be an object")
                request = cast(dict, request)
                requests.append(request)
                send(runtime_stdin, request)
                response = read_message(runtime_stdout, remaining)
                responses.append(response)
                send(agent_stdin, {"type": "response", "response": response})
            elif message_type == "done":
                usage = message.get("usage")
                break
            else:
                raise RuntimeError(
                    f"unknown Agent session message type: {message_type!r}"
                )
        return {
            "id": task["id"],
            "requests": requests,
            **({"usage": usage} if usage else {}),
        }
    finally:
        if runtime.stdin:
            runtime.stdin.close()
        try:
            runtime.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            runtime.kill()
            runtime.wait()
        if runtime.returncode:
            detail = runtime.stderr.read() if runtime.stderr else ""
            raise RuntimeError(f"Raccord runtime failed for {task['id']}: {detail}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--agent-command",
        required=True,
        help="command implementing the Agent session protocol",
    )
    parser.add_argument("--fixture", type=Path, default=FIXTURE)
    parser.add_argument("--transcript-output", type=Path, required=True)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--timeout", type=float, default=30.0)
    args = parser.parse_args()

    tasks = load_tasks(args.fixture)
    agent = subprocess.Popen(
        shlex.split(args.agent_command),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    require(
        agent.stdin is not None and agent.stdout is not None, "Agent pipes unavailable"
    )
    transcript = []
    try:
        for task in tasks:
            transcript.append(run_task(agent, task, args.timeout))
    finally:
        if agent.stdin:
            agent.stdin.close()
        try:
            agent.wait(timeout=args.timeout)
        except subprocess.TimeoutExpired:
            agent.kill()
            agent.wait()
    require(
        agent.returncode == 0, f"Agent process failed with exit code {agent.returncode}"
    )

    try:
        args.transcript_output.write_text(
            json.dumps(transcript, indent=2) + "\n",
            encoding="utf-8",
        )
    except OSError as error:
        raise RuntimeError(
            f"cannot write transcript: {args.transcript_output}"
        ) from error

    command = [
        sys.executable,
        str(ROOT / "scripts" / "run-agent-benchmark.py"),
        "--fixture",
        str(args.fixture),
        "--transcript",
        str(args.transcript_output),
    ]
    if args.report:
        command.extend(("--report", str(args.report)))
    subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
