#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
transcript="$(mktemp)"
trap 'rm -f "$transcript"' EXIT

mise exec -- cargo run --quiet \
    --manifest-path "$root/Cargo.toml" \
    --bin jsonl_api >"$transcript" <<'JSONL'
{"tool":"find","args":{"query":"asset://b","limit":5}}
{"tool":"inspect","args":{"ids":["b"]}}
{"tool":"plan_edit","args":{"base_revision":0,"edits":[{"op":"ripple_delete","clip_id":"b"}]}}
{"tool":"commit_edit","args":{"plan_token":"plan-1"}}
{"tool":"verify"}
{"tool":"plan_edit","args":{"base_revision":0,"edits":[]}}
JSONL

cat "$transcript"

python3 - "$transcript" <<'PY'
import json
import sys


def require(condition, detail):
    if not condition:
        raise RuntimeError(detail)


with open(sys.argv[1], encoding="utf-8") as stream:
    responses = [json.loads(line) for line in stream if line.strip()]

require(len(responses) == 6, "expected six JSONL responses")
require(responses[0]["result"]["matches"][0]["id"] == "b", "find did not return clip b")
require(responses[1]["result"]["clips"][0]["id"] == "b", "inspect projection is wrong")
require(
    responses[2]["result"]["placements"]
    == [
        {"id": "a", "start_frame": 0, "duration_frames": 72},
        {"id": "c", "start_frame": 72, "duration_frames": 96},
    ],
    "ripple placements are wrong",
)
require(responses[3]["result"]["version"] == 1, "commit version is wrong")
require(responses[4]["result"] == {"pass": True, "version": 1}, "verification failed")
require(responses[5]["error"]["code"] == "STALE_REVISION", "stale revision was not rejected")

print("validated JSONL find/inspect/plan/commit/verify/stale flow")
PY
