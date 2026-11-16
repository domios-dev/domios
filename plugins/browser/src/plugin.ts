import { defineProviderIo, ioResolve, type HostIoSteps, type IoExports, type AgentCommandCtx, type PluginModule } from "@codeterm/plugin-sdk";
import { readPage } from "./readPage";
import { redactCredentials } from "./redaction";
import { runBatch } from "./batch";
import { version } from "../plugin.json";

export type AgentCommandReply = { result: string } | { error: string };

type SurfaceCall = (verb: string, argsJson: string) => string | HostIoSteps<string>;

export type ParsedArgs = { args: string[]; of?: string } | { error: string };

function hostSurfaceCall(): SurfaceCall | undefined {
  return typeof host === "object" && host && host.io?.surface ? host.io.surface.call : undefined;
}

const OF_USAGE = "--of needs an agent name or tab id";

/** Pulls `--of <owner>` (or `--of=<owner>`) out of the verb's words; the core resolves and authorizes it. */
export function parseOf(raw: readonly unknown[]): ParsedArgs {
  const args: string[] = [];
  let of: string | undefined;
  for (let index = 0; index < raw.length; index += 1) {
    const word = String(raw[index]);
    let value: string | undefined;
    if (word === "--of") {
      index += 1;
      value = index < raw.length ? String(raw[index]) : "";
    } else if (word.startsWith("--of=")) {
      value = word.slice("--of=".length);
    } else {
      args.push(word);
      continue;
    }
    if (!value.trim()) return { error: OF_USAGE };
    if (of !== undefined) return { error: "--of can name only one agent" };
    of = value.trim();
  }
  return of === undefined ? { args } : { args, of };
}

// The core browser runtime does the work; the plugin forwards the agent's verb to its own surface or `--of` another's.
export function* forward(ctx: AgentCommandCtx, call: SurfaceCall | undefined = hostSurfaceCall()): HostIoSteps<AgentCommandReply> {
  if (!call) return { error: "host.surface is unavailable in this Domios build" };
  if (ctx.verb === "batch") return yield* runBatch(ctx, (step) => forward(step, call));
  if (ctx.verb === "connect-secret") return { error: "connect accepts its token only through --stdin" };
  const secret = ctx.verb === "connect" ? ctx.body?.trim() : ctx.body;
  const verb = secret !== undefined && ctx.verb === "type" ? "type-secret" : ctx.verb === "connect" ? "connect-secret" : ctx.verb === "attach" ? "open" : ctx.verb === "--help" ? "help" : ctx.verb || "help";
  if (secret !== undefined && !["type", "connect"].includes(ctx.verb)) return { error: "Browser body input is supported only by type and connect" };
  if (ctx.verb === "connect" && !secret?.trim()) return { error: "connect requires --stdin with a nonempty token" };
  const parsed = parseOf(Array.isArray(ctx.args) ? ctx.args : []);
  if ("error" in parsed) return { error: parsed.error };
  if (parsed.args.includes("--help")) return { error: "Use `domios plugin browser help` for browser verbs and syntax" };
  const redact = (text: string) => redactCredentials(secret ? text.split(secret).join("REDACTED") : text);
  if (ctx.verb === "connect") {
    try {
      const opening = JSON.parse(yield* ioResolve(call("open", JSON.stringify(parsed.of === undefined ? parsed.args : { args: parsed.args, of: parsed.of })))) as { result?: string; error?: string };
      if (typeof opening.error === "string") return { error: redact(opening.error) };
      if (typeof opening.result !== "string") return { error: "the browser runtime did not confirm the requested session" };
    } catch { return { error: "the browser runtime could not open the requested session" }; }
  }
  if (secret !== undefined) parsed.args.push(secret);
  const payload = parsed.of === undefined ? parsed.args : { args: parsed.args, of: parsed.of };
  let reply: unknown;
  try {
    reply = JSON.parse(yield* ioResolve(call(verb, JSON.stringify(payload))));
  } catch {
    return { error: `the browser runtime returned no answer for ${verb}` };
  }
  if (reply && typeof reply === "object") {
    const { result, error } = reply as { result?: unknown; error?: unknown };
    const recovery = parsed.of === undefined ? "domios plugin browser snapshot" : `domios plugin browser snapshot --of ${JSON.stringify(parsed.of)}`;
    if (typeof error === "string") return { error: redact(error.replaceAll("domios plugin browser snapshot", recovery)) };
    if (typeof result === "string") return { result: redact(verb === "help" ? `Browser plugin ${version}\n${result}` : result) };
  }
  return { error: `the browser runtime returned no answer for ${verb}` };
}

const plugin: IoExports<PluginModule> = {
  onAgentCommand: (ctx) => forward(ctx),
  blueprintSubmit: (request) => readPage(request, hostSurfaceCall()),
};

export default defineProviderIo(plugin, () => host.io);
