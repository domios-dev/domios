import type { BlueprintJobUpdate, BlueprintStepDescriptor, BlueprintStepRequest, HostAgentRun } from "@codeterm/plugin-sdk";

export function runPromptDescriptor(id: string, title: string): BlueprintStepDescriptor {
  return {
    contractVersion: 1, revision: 1, id: `${id}.run_prompt`, title: `${title}: run prompt`,
    description: "Runs one prompt in a separate agent tab using this provider. Lost runs are never resubmitted.",
    params: { type: "object", additionalProperties: false, required: ["prompt", "cwd"], properties: {
      prompt: { type: "string", maxLength: 32768 }, model: { type: "string", maxLength: 128 },
      cwd: { type: "string", maxLength: 1024 }, timeout_ms: { type: "integer", minimum: 1, maximum: 3600000, default: 600000 },
    } },
    inputs: { context: { kind: "text", minItems: 0, maxItems: 1 } },
    outputs: { text: { kind: "text", minItems: 1, maxItems: 1 } },
    outcomes: ["done", "failed"], lifecycle: { reconcile: true, cancel: true }, effects: { sideEffects: true },
  };
}

function bytes(text: string): number {
  let count = 0;
  for (const point of text) { const code = point.codePointAt(0)!; count += code < 128 ? 1 : code < 2048 ? 2 : code < 65536 ? 3 : 4; }
  return count;
}

function bounded(text: string): string {
  if (bytes(JSON.stringify(text)) <= 60 * 1024) return text;
  let points = Array.from(text);
  while (bytes(JSON.stringify(points.join("") + "\n[truncated]")) > 60 * 1024) points = points.slice(0, Math.floor(points.length * 0.9));
  return points.join("") + "\n[truncated]";
}

export function runPromptSteps(getHost: () => HostAgentRun, options: { sandbox?: "read-only" } = {}) {
  function update(job: string, reply: { state?: string; text?: string; reason?: string; error?: string }): BlueprintJobUpdate {
    if (reply.state === "running") return { job, state: "running", outcome: null, outputs: {} };
    if (reply.state === "unknown" || reply.state === "canceled") return { job, state: reply.state, outcome: null, outputs: {} };
    const done = reply.state === "done" && typeof reply.text === "string" && reply.text.trim().length > 0;
    return { job, state: "succeeded", outcome: done ? "done" : "failed", outputs: { text: [{ kind: "text", text: bounded(done ? reply.text! : "The agent run failed.") }] } };
  }
  return {
    blueprintSubmit(request: BlueprintStepRequest): BlueprintJobUpdate {
      const params = request.params;
      const failed = () => update(request.context.operation, { state: "failed" });
      if (typeof params.prompt !== "string" || !params.prompt.trim() || params.prompt.length > 32768
          || typeof params.cwd !== "string" || params.cwd.length > 1024 || !/^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(params.cwd)
          || (params.model !== undefined && (typeof params.model !== "string" || !params.model.trim() || params.model.length > 128))) return failed();
      const timeout = params.timeout_ms ?? 600000;
      if (!Number.isInteger(timeout) || (timeout as number) < 1 || (timeout as number) > 3600000) return failed();
      const context = request.inputs.context?.[0];
      let prompt = params.prompt;
      if (context) {
        if (context.kind !== "text") return failed();
        const fence = "`".repeat(Math.max(3, ...Array.from(context.text.matchAll(/`+/g), match => match[0].length + 1)));
        prompt += `\n\nContext:\n${fence}text\n${context.text}\n${fence}`;
      }
      if (bytes(prompt) > 48 * 1024) return failed();
      return getHost().start({ prompt, cwd: params.cwd, model: params.model as string | undefined, timeoutMs: timeout as number, ...options }, (reply): BlueprintJobUpdate => {
        if (reply.error === "launch_unconfirmed" || reply.error === "transport_unconfirmed") return update(request.context.operation, { state: "unknown" });
        return typeof reply.run === "string" ? { job: reply.run, state: "running", outcome: null, outputs: {} } : failed();
      });
    },
    blueprintReconcile(request: BlueprintStepRequest): BlueprintJobUpdate {
      const job = request.job ?? request.context.operation;
      return getHost().await(job, reply => update(job, reply));
    },
    blueprintCancel(request: BlueprintStepRequest): BlueprintJobUpdate {
      const job = request.job ?? request.context.operation;
      return getHost().cancel(job, reply => update(job, reply));
    },
  };
}
