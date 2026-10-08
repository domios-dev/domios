import type { AgentCommandCtx, PluginModule } from "@codeterm/plugin-sdk";

export type AgentCommandReply = { result: string } | { error: string };

type SurfaceCall = (verb: string, argsJson: string) => string;

function hostSurfaceCall(): SurfaceCall | undefined {
  return typeof host === "object" && host && host.surface ? host.surface.call : undefined;
}

// The core browser runtime does the work; the plugin forwards the agent's verb to its own surface.
export function forward(ctx: AgentCommandCtx, call: SurfaceCall | undefined = hostSurfaceCall()): AgentCommandReply {
  if (!call) return { error: "host.surface is unavailable in this Domios build" };
  const verb = ctx.verb || "help";
  let reply: unknown;
  try {
    reply = JSON.parse(call(verb, JSON.stringify(Array.isArray(ctx.args) ? ctx.args : [])));
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
