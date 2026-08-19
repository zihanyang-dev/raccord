# Experiment 01: Product Path

This package is intentionally **outside the Raccord workspace**. It is a disposable
experiment for validating the product path before changing the formal IR.

## Hypothesis

An Agent can edit a short timeline accurately with a small semantic command set:

```text
stable clip IDs + semantic anchors + plan/commit/verify
```

The Agent must not provide absolute frame positions, array indexes, or FFmpeg
expressions.

## Current scenario

```text
24 fps
A: 72 frames
B: 72 frames
C: 96 frames

Operation: ripple-delete B
Expected: A = 0..72, C = 72..168
```

## Run

First generate deterministic synthetic media with Homebrew FFmpeg:

```bash
./experiments/product-path/scripts/generate-fixture.sh
```

Run the semantic model tests and preview:

```bash
mise exec -- cargo test --manifest-path experiments/product-path/Cargo.toml
mise exec -- cargo run --manifest-path experiments/product-path/Cargo.toml \
  --bin raccord-product-path-experiment
```

Run the compact JSONL tool protocol:

```bash
./experiments/product-path/scripts/run-jsonl-scenario.sh
```

The protocol currently exposes only `find`, `inspect`, `plan_edit`, `commit_edit`,
and `verify`. The JSONL transcript is the first stand-in for a real Agent tool
loop.
Run the deterministic scenario suite with:

```bash
python3 experiments/product-path/scripts/run-jsonl-suite.py
```

The suite covers ripple delete, source replacement plus trim, insert plus move,
and structured missing-clip errors.

Replay ten Agent-style reference tasks and report protocol metrics with:

```bash
python3 experiments/product-path/scripts/run-agent-benchmark.py \
  --report /tmp/raccord-agent-benchmark.json
```

A future LLM-generated transcript can be replayed with:

```bash
python3 experiments/product-path/scripts/run-agent-benchmark.py \
  --transcript /path/to/agent-transcript.json
```

The transcript only needs task ids and generated `requests`; expected results
remain in the benchmark fixture. This currently measures tool calls, protocol
bytes, and structured recovery errors. The checked-in transcripts are reference
transcripts, not LLM-generated runs, so actual token usage is intentionally not
reported yet.

For an interactive Agent process, use the session router:

```bash
python3 experiments/product-path/scripts/run-agent-session.py \
  --agent-command "python3 experiments/product-path/scripts/mock-agent.py \
    --fixture experiments/product-path/fixtures/agent-benchmark.json" \
  --transcript-output /tmp/raccord-session.json \
  --report /tmp/raccord-session-report.json
```

The session protocol is:

```text
router → {type: task, prompt, id}
Agent  → {type: request, request: {...}}
router → {type: response, response: {...}}
Agent  → {type: done}
```

`mock-agent.py` only replays reference requests; it is a protocol test, not an
LLM. For a feedback-driven deterministic baseline, use:

```bash
python3 experiments/product-path/scripts/run-agent-session.py \
  --agent-command "python3 experiments/product-path/scripts/policy-agent.py" \
  --transcript-output /tmp/raccord-policy-transcript.json \
  --report /tmp/raccord-policy-report.json
```

`policy-agent.py` discovers clip IDs with `find`, reads revisions with
`inspect`, uses returned plan tokens, and branches on `MISSING_CLIP` and
`STALE_REVISION`. A real Agent command can replace it without changing the
runtime router or validator.

Run the real local Pi Agent through the same extension and router:

```bash
python3 experiments/product-path/scripts/run-pi-agent-benchmark.py \
  --model openai-codex/gpt-5.4-mini \
  --runs 2 \
  --transcript-output /tmp/raccord-pi-transcript.json \
  --report /tmp/raccord-pi-report.json
```

The initial seven-task Pi benchmark completed successfully with 40 tool calls,
one recoverable stale-revision error, and 31,511 provider-reported tokens. The
fixture now contains ten tasks, including duplicate metadata, invalid duration,
and self-anchor recovery. Two consecutive ten-task Pi runs completed all 20 task
executions, with 58 and 57 tool calls, 55,999 and 54,445 provider-reported tokens,
and the expected structured recovery errors. This is a real Agent measurement,
not a deterministic reference replay. Failed runs are recorded in the aggregate
report instead of being silently counted as success.

Generate and render the 48-second, six-clip fixture:

```bash
bash experiments/product-path/scripts/generate-long-fixture.sh
bash experiments/product-path/scripts/render-long-fixture.sh
```

The long render validates 48.000 seconds, 1,152 video frames, a -3 dB gain on
clip `d`, and an anchored subtitle on clip `b`. Pi can use the same timeline:

```bash
python3 experiments/product-path/scripts/run-pi-agent-benchmark.py \
  --fixture experiments/product-path/fixtures/long-agent-benchmark.json \
  --timeline experiments/product-path/fixtures/long-timeline.json \
  --model openai-codex/gpt-5.4-mini \
  --runs 2 \
  --transcript-output /tmp/raccord-pi-long.json \
  --report /tmp/raccord-pi-long-report.json
```

Two long-timeline Pi runs completed all six task executions, with 18 tool calls
and about 18.2k provider-reported tokens per run.

Validate artifact reuse on the long timeline:

```bash
python3 experiments/product-path/scripts/run-long-cache-experiment.py
```

The long artifact cache produced a miss, a hit, a marker-only hit, and a new
media key for the audio-gain change. It uses the formal `ArtifactStore` rather
than a Python-only file copy.

Validate per-clip partial rendering:

```bash
python3 experiments/product-path/scripts/run-long-partial-render-experiment.py
```

The first six clip segments miss, a marker-only edit reuses all six, and the
changed audio gain re-renders exactly one segment while reusing the other five.
A marker-only change also reuses the subtitle overlay, while a subtitle text
change rebuilds only that overlay. Each final output remains about 48 seconds
and 1,152 frames.

Validate semantic transitions:

```bash
python3 experiments/product-path/scripts/run-transition-experiment.py
```

The transition experiment plans a one-second crossfade from `b` to adjacent `c`,
renders 23 seconds and 552 frames with FFmpeg `xfade`, and verifies that the
transition changes composite and transition keys without changing clip-local
media keys. Two Pi runs also completed the corresponding semantic transition
task with six tool calls per run.

The metadata path is now connected to an experimental FFmpeg renderer:

```bash
./experiments/product-path/scripts/render-metadata.sh
```

It validates a nine-second three-clip output, a -3 dB gain delta on clip `b`,
and writes a frame showing the anchored `Hello` subtitle. This renderer is still
an experiment and uses a small bitmap subtitle fallback because the installed
FFmpeg build does not expose `drawtext`.

Measure deterministic cache invalidation boundaries:

```bash
python3 experiments/product-path/scripts/run-cache-experiment.py
```

The current result shows that a marker-only change reuses both the media render
key and subtitle overlay key, while an audio gain change invalidates only the
media render key. The keys are now generated by the typed Rust cache module and
exposed through the experimental `cache_keys` runtime query.

Exercise actual artifact reuse with:

```bash
python3 experiments/product-path/scripts/run-artifact-cache-experiment.py
```

The current run produces one cache miss, one cache hit, and a marker-only cache
hit; the changed audio-gain key has no existing artifact. Publication and lookup
now route through the formal `raccord-cache::ArtifactStore` API, including its
manifest, atomic file publication, and RAII key lock with stale-lock recovery.

Render the result of the semantic `ripple_delete` operation:

```bash
./experiments/product-path/scripts/render-ripple-delete.sh
```

The output should be approximately six seconds and contain clip `a` followed by
clip `c`; clip `b` must not appear.

## Measurements to add

- first-pass edit success;
- invalid commit count;
- tool calls per edit;
- tokens per edit;
- affected range versus total range;
- cache reuse after a local change;
- deterministic plan hash.

## Promotion rule

Nothing in this experiment becomes part of the formal Raccord IR automatically.
A behavior is promoted only after it has fixtures, diagnostics, and a stable
semantic contract.
