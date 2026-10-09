import type { AgentCommandCtx, PluginModule } from "@codeterm/plugin-sdk";

export type AgentCommandReply = { result: string } | { error: string };

type SurfaceCall = (verb: string, argsJson: string) => string;

export type ParsedArgs = { args: string[]; of?: string } | { error: string };

function hostSurfaceCall(): SurfaceCall | undefined {
  return typeof host === "object" && host && host.surface ? host.surface.call : undefined;
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
export function forward(ctx: AgentCommandCtx, call: SurfaceCall | undefined = hostSurfaceCall()): AgentCommandReply {
  if (!call) return { error: "host.surface is unavailable in this Domios build" };
  const verb = ctx.verb || "help";
  const parsed = parseOf(Array.isArray(ctx.args) ? ctx.args : []);
  if ("error" in parsed) return { error: parsed.error };
  const payload = parsed.of === undefined ? parsed.args : { args: parsed.args, of: parsed.of };
  let reply: unknown;
  try {
    reply = JSON.parse(call(verb, JSON.stringify(payload)));
  } catch {
    return { error: `the browser runtime returned no answer for ${verb}` };
  }
  if (reply && typeof reply === "object") {
    const { result, error } = reply as { result?: unknown; error?: unknown };
    if (typeof error === "string") return { error };
    if (typeof result === "string") return { result };
  }
  return { error: `the browser runtime returned no answer for ${verb}` };
}

const plugin: PluginModule = {
  onAgentCommand: (ctx) => forward(ctx),
};

export default plugin;
