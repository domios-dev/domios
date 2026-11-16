import { ioResolve, type HostIoSteps, type AgentCommandCtx } from "@codeterm/plugin-sdk";
import type { AgentCommandReply } from "./plugin";

type Step = { name: string; input: { args: string[] } };
const MAX_STEPS = 32;
const MAX_BODY = 64 * 1024;

function parse(body: string | undefined): Step[] {
  if (!body || body.length > MAX_BODY) throw new Error("batch requires JSON through --from-pad or stdin (maximum 64 KiB)");
  const steps: unknown = JSON.parse(body);
  if (!Array.isArray(steps) || !steps.length || steps.length > MAX_STEPS) throw new Error("batch requires 1 to 32 steps");
  return steps.map((step: unknown) => {
    if (!step || typeof step !== "object") throw new Error("each batch step requires {name, input: {args}}");
    const { name, input } = step as { name?: unknown; input?: unknown };
    if (typeof name !== "string" || !/^[a-z][a-z-]*$/.test(name) || ["batch", "type-secret", "connect-secret"].includes(name)) throw new Error("invalid or nested batch verb");
    if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("batch input requires args");
    const args = (input as { args?: unknown }).args;
    if (Object.keys(step).some((key) => !["name", "input"].includes(key)) || Object.keys(input).some((key) => key !== "args") || !Array.isArray(args) || args.some((arg) => typeof arg !== "string")) throw new Error("batch input accepts only a string args array; secrets must use standalone stdin");
    return { name, input: { args } };
  });
}

export function* runBatch(ctx: AgentCommandCtx, execute: (ctx: AgentCommandCtx) => AgentCommandReply | HostIoSteps<AgentCommandReply>): HostIoSteps<AgentCommandReply> {
  let steps: Step[];
  try { steps = parse(ctx.body); } catch { return { error: "invalid batch: expected 1 to 32 {name, input: {args: string[]}} steps, at most 64 KiB; no nesting or secret verbs" }; }
  if (ctx.args?.length) return { error: "batch takes no positional arguments; put --of in each step's args" };
  const results: { name: string; result?: string; error?: string }[] = [];
  for (const step of steps) {
    let reply: AgentCommandReply;
    try { reply = yield* ioResolve(execute({ ...ctx, verb: step.name, args: step.input.args, body: undefined })); }
    catch { reply = { error: "action_unconfirmed: runtime interrupted; inspect before continuing; never replay" }; }
    results.push({ name: step.name, ...reply });
    if ("error" in reply || ("result" in reply && /\b(?:action|upload|state)_unconfirmed\b/.test(reply.result))) return { result: JSON.stringify({ completed: false, stoppedAt: results.length - 1, results }) };
  }
  return { result: JSON.stringify({ completed: true, results }) };
}
