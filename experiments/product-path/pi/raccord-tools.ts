import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { resolve } from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

type RuntimeResponse = {
  ok: boolean;
  result?: Record<string, unknown> | null;
  error?: { code: string; detail: string } | null;
};

type PendingResponse = {
  resolve: (response: RuntimeResponse) => void;
  reject: (error: Error) => void;
};

const root =
  process.env.RACCORD_PRODUCT_PATH ??
  resolve(process.cwd(), "experiments/product-path");
const manifest = resolve(root, "Cargo.toml");

let runtime: ChildProcessWithoutNullStreams | undefined;
let outputBuffer = "";
let pending: PendingResponse[] = [];
let runtimeError: Error | undefined;

function rejectPending(error: Error): void {
  runtimeError = error;
  const waiting = pending;
  pending = [];
  for (const item of waiting) item.reject(error);
}

function startRuntime(): void {
  if (runtime) return;

  const runtimeArguments = [
    "exec",
    "--",
    "cargo",
    "run",
    "--quiet",
    "--manifest-path",
    manifest,
    "--bin",
    "jsonl_api",
  ];
  const timeline = process.env.RACCORD_TIMELINE;
  if (timeline) runtimeArguments.push("--", "--timeline", timeline);

  runtime = spawn("mise", runtimeArguments, { cwd: root });
  runtime.stdout.setEncoding("utf8");
  runtime.stdout.on("data", (chunk: string) => {
    outputBuffer += chunk;
    while (true) {
      const newline = outputBuffer.indexOf("\n");
      if (newline < 0) break;
      const line = outputBuffer.slice(0, newline).trim();
      outputBuffer = outputBuffer.slice(newline + 1);
      if (!line) continue;
      const waiter = pending.shift();
      if (!waiter) {
        rejectPending(new Error(`Unexpected Raccord runtime output: ${line}`));
        return;
      }
      try {
        waiter.resolve(JSON.parse(line) as RuntimeResponse);
      } catch (error) {
        waiter.reject(
          new Error(`Invalid Raccord runtime JSON: ${String(error)}`),
        );
      }
    }
  });
  runtime.stderr.setEncoding("utf8");
  runtime.on("error", (error) => rejectPending(error));
  runtime.on("exit", (code, signal) => {
    runtime = undefined;
    if (code !== 0 || signal) {
      rejectPending(
        new Error(`Raccord runtime exited: code=${code}, signal=${signal}`),
      );
    }
  });
}

async function callRuntime(
  tool: string,
  args: Record<string, unknown> = {},
): Promise<RuntimeResponse> {
  startRuntime();
  if (runtimeError) throw runtimeError;
  if (!runtime?.stdin) throw new Error("Raccord runtime is unavailable");

  const response = new Promise<RuntimeResponse>(
    (resolveResponse, rejectResponse) => {
      pending.push({ resolve: resolveResponse, reject: rejectResponse });
    },
  );
  runtime.stdin.write(`${JSON.stringify({ tool, args })}\n`);
  return response;
}

function resultText(response: RuntimeResponse): {
  content: [{ type: "text"; text: string }];
  details: unknown;
} {
  if (!response.ok) {
    const error = response.error ?? {
      code: "UNKNOWN_RUNTIME_ERROR",
      detail: "unknown error",
    };
    throw new Error(`${error.code}: ${error.detail}`);
  }
  const result = response.result ?? {};
  return {
    content: [{ type: "text", text: JSON.stringify(result) }],
    details: result,
  };
}

const clip = Type.Object({
  id: Type.String(),
  source: Type.String(),
  duration_frames: Type.Integer({ minimum: 1 }),
  audio_gain_db_milli: Type.Optional(Type.Integer()),
});

const edit = Type.Object({
  op: Type.String({
    description:
      "ripple_delete, replace_source, insert_after, move_after, trim, set_audio_gain, add_marker, add_subtitle, or add_transition",
  }),
  id: Type.Optional(Type.String()),
  clip_id: Type.Optional(Type.String()),
  source: Type.Optional(Type.String()),
  duration_frames: Type.Optional(Type.Integer({ minimum: 1 })),
  gain_db_milli: Type.Optional(Type.Integer()),
  label: Type.Optional(Type.String()),
  text: Type.Optional(Type.String()),
  after: Type.Optional(Type.Union([Type.String(), Type.Null()])),
  from_clip_id: Type.Optional(Type.String()),
  to_clip_id: Type.Optional(Type.String()),
  kind: Type.Optional(Type.String()),
  clip: Type.Optional(clip),
});

function register(pi: ExtensionAPI): void {
  pi.registerTool({
    name: "raccord_find",
    label: "Raccord Find",
    description: "Find timeline clips by stable ID or source reference.",
    promptSnippet: "Find timeline clips by semantic query",
    parameters: Type.Object({
      query: Type.String(),
      limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 20 })),
    }),
    async execute(_toolCallId, params) {
      return resultText(await callRuntime("find", params));
    },
  });

  pi.registerTool({
    name: "raccord_inspect",
    label: "Raccord Inspect",
    description: "Inspect the current revision and selected timeline clips.",
    promptSnippet: "Inspect selected timeline clips and revision",
    parameters: Type.Object({ ids: Type.Optional(Type.Array(Type.String())) }),
    async execute(_toolCallId, params) {
      return resultText(await callRuntime("inspect", params));
    },
  });

  pi.registerTool({
    name: "raccord_plan_edit",
    label: "Raccord Plan Edit",
    description:
      "Plan semantic timeline edits; never provide absolute frame positions.",
    promptSnippet: "Plan semantic timeline edits",
    parameters: Type.Object({
      base_revision: Type.Integer({ minimum: 0 }),
      edits: Type.Array(edit),
    }),
    async execute(_toolCallId, params) {
      return resultText(await callRuntime("plan_edit", params));
    },
  });

  pi.registerTool({
    name: "raccord_commit_edit",
    label: "Raccord Commit Edit",
    description:
      "Commit a previously validated Raccord plan using its returned plan token.",
    promptSnippet: "Commit a validated semantic edit plan",
    parameters: Type.Object({ plan_token: Type.String() }),
    async execute(_toolCallId, params) {
      return resultText(await callRuntime("commit_edit", params));
    },
  });

  pi.registerTool({
    name: "raccord_verify",
    label: "Raccord Verify",
    description: "Verify timeline invariants after a semantic edit.",
    promptSnippet: "Verify timeline invariants",
    parameters: Type.Object({}),
    async execute(_toolCallId) {
      return resultText(await callRuntime("verify"));
    },
  });
}

export default function (pi: ExtensionAPI): void {
  pi.on("session_start", () => {
    startRuntime();
  });
  pi.on("session_shutdown", () => {
    if (runtime) runtime.kill();
    runtime = undefined;
  });
  register(pi);
}
