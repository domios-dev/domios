import {
  ioValue,
  type AgentTaskInput,
  type AgentTaskResult,
  type HostIoResult,
  type HostIoSteps,
} from "@codeterm/plugin-sdk";

type Usage = { input_tokens: number; cached_input_tokens: number; output_tokens: number };
type ExecResult = HostIoResult<"execPoll">;

type Run = { deadline: number; job?: string; cancelled: boolean };
const runs = new Map<string, Run>();

function failed(error: string): AgentTaskResult {
  return { status: "failed", output: null, error };
}

function stopped(run: Run): AgentTaskResult | undefined {
  if (run.cancelled) return { status: "cancelled", output: null };
  if (host.unixNowMs() >= run.deadline) return { status: "deadline_exceeded", output: null };
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function readUsage(value: Partial<Usage> | undefined): Usage | undefined {
  if (isCount(value?.input_tokens) && isCount(value?.output_tokens)) {
    return {
      input_tokens: value.input_tokens,
      cached_input_tokens: isCount(value.cached_input_tokens) ? value.cached_input_tokens : 0,
      output_tokens: value.output_tokens,
    };
  }
}

function taskOutput(text: string, threadId: string, usage: Usage | undefined): AgentTaskResult["output"] {
  // Preserve typed JSON so the host can validate agent-review verdicts.
  try {
    const parsed = JSON.parse(text) as AgentTaskResult["output"];
    if (parsed !== null && typeof parsed === "object") return parsed;
  } catch {
    // Codex may answer in plain text.
  }
  return { text, threadId, ...(usage && { usage }) };
}

function readResult(result: ExecResult, maxTokens: number): AgentTaskResult {
  if (/^codex timed out after \d+ms$/.test(result.error ?? "")) {
    return { status: "deadline_exceeded", output: null };
  }
  if (result.truncated || result.outputTooLarge) return failed("Codex task transcript exceeded the capture limit");
  if (result.done !== true || result.code !== 0 || result.error) {
    return failed(result.error?.slice(0, 512) ?? "Codex task process failed");
  }

  let threadId: string | undefined;
  let text: string | undefined;
  let completed = false;
  let usage: Usage | undefined;
  for (const line of (result.stdout ?? "").split("\n")) {
    if (!line.trim()) continue;
    const event = JSON.parse(line) as {
      type: string;
      thread_id?: string;
      item?: { type: string; text?: string };
      usage?: Partial<Usage>;
    };
    switch (event.type) {
      case "thread.started":
        if (threadId || !event.thread_id) return failed("Unexpected Codex task thread");
        threadId = event.thread_id;
        break;
      case "turn.completed":
        if (completed) return failed("Unexpected extra Codex task turn");
        completed = true;
        usage = readUsage(event.usage);
        break;
      case "turn.failed":
        return failed("Codex task turn failed");
      case "item.completed":
        if (event.item?.type === "agent_message" && typeof event.item.text === "string") {
          text = event.item.text;
        }
        break;
    }
  }
  if (!threadId || !completed || !text?.trim()) return failed("Codex task completed without a final answer");
  if (usage && usage.input_tokens + usage.output_tokens > maxTokens) return failed("Codex task token budget exceeded");
  const output = taskOutput(text, threadId, usage);
  // UTF-16 length times three bounds the UTF-8 JSON payload, leaving room for the result wrapper.
  if (JSON.stringify(output).length * 3 > 65_000) return failed("Codex task result exceeds the output limit");
  return { status: "done", output };
}

export function* runAgentTask(input: AgentTaskInput): HostIoSteps<AgentTaskResult> {
  const now = host.unixNowMs();
  for (const [id, run] of runs) {
    if (run.deadline <= now) runs.delete(id);
  }
  if (runs.has(input.runId)) return failed("Codex task is already running");
  const cwd = host.homeDir();
  if (!cwd) return failed("Codex task working directory unavailable");
  if (!/^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$/.test(input.profile)) return failed("Invalid Codex task model");
  const run: Run = { deadline: now + input.timeoutMs, cancelled: false };
  runs.set(input.runId, run);
  try {
    const { jobId, error } = yield* ioValue<HostIoResult<"execStart">>({
      op: "execStart",
      options: {
        bin: "codex",
        args: [
          "exec", "--ephemeral", "--json", "--ignore-user-config", "--skip-git-repo-check",
          "--cd", cwd, "--sandbox", "read-only", "--model", input.profile,
          "--disable", "hooks", "--disable", "plugins",
          "--disable", "shell_tool", "--disable", "unified_exec",
          "--disable", "multi_agent", "--disable", "multi_agent_v2",
          "-c", 'approval_policy="never"', "-c", 'web_search="disabled"',
          "-c", "project_doc_max_bytes=0", "-",
        ],
        stdin: input.input ? `${input.task}\n\n${input.input}` : input.task,
        timeoutMs: input.timeoutMs,
      },
    });
    if (jobId) run.job = jobId;
    const interrupted = stopped(run);
    if (interrupted) {
      if (run.job) yield* ioValue({ op: "execClose", job: run.job });
      return interrupted;
    }
    if (!jobId) return failed(error?.slice(0, 512) ?? "Codex task could not start");
    const result = yield* ioValue<ExecResult>({ op: "awaitJob", job: jobId });
    return stopped(run) ?? readResult(result, input.maxTokens);
  } catch {
    return stopped(run) ?? failed("Codex task execution or transcript parsing failed");
  } finally {
    if (runs.get(input.runId) === run) runs.delete(input.runId);
  }
}

export function* abortAgentTask(runId: string): HostIoSteps<null> {
  const run = runs.get(runId);
  if (run) {
    run.cancelled = true;
    runs.delete(runId);
    if (run.job) yield* ioValue({ op: "execClose", job: run.job });
  }
  return null;
}
