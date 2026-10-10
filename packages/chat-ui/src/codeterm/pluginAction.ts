import type { CodetermInvocation } from "./recognize";

/** Resolve an installed plugin id to its display name. Returns undefined when unknown. */
export type PluginNameResolver = (pluginId: string) => string | undefined;

const LEAD: Record<string, string> = {
  get: "plugin",
  config: "configure",
  install: "install",
  update: "update",
  "update-system": "update",
  remove: "remove",
  "receiver-target": "receiver",
  settings: "settings",
  validate: "validate",
};

function folderName(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;
}

function isPath(value: string): boolean {
  return /[\\/]/.test(value);
}

export function pluginLabel(pluginId: string, resolve?: PluginNameResolver): string {
  return resolve?.(pluginId) ?? pluginId;
}

const VERB_ROW_MAX = 90;

function verbArg(value: string): string {
  const short = value.replace(/^https?:\/\//, "").replace(/\/$/, "");
  return /\s/.test(short) ? `"${short}"` : short;
}

/** `codeterm plugin <id> <verb> …` (an agent verb) reads as the plugin name plus what it was asked to do. */
function describeAgentVerb(inv: CodetermInvocation, resolve?: PluginNameResolver): { lead: string; object: string } | null {
  const [pluginId, verb, ...args] = inv.positionals;
  if (!pluginId || !verb) return null;
  const flags = Object.entries(inv.flags).map(([key, value]) => (value === true ? `--${key}` : `--${key} ${verbArg(value)}`));
  const name = pluginLabel(pluginId, resolve);
  const call = [verb, ...args.map(verbArg), ...flags].join(" ");
  return { lead: name, object: call.length > VERB_ROW_MAX ? `${call.slice(0, VERB_ROW_MAX - 1)}…` : call };
}

/** A `codeterm plugin …` row reads as a verb and the plugin's display name, never its id or a folder path. */
export function describePluginAction(inv: CodetermInvocation, resolve?: PluginNameResolver): { lead: string; object?: string } {
  const action = inv.path[1];
  if (!action) {
    const verbRow = describeAgentVerb(inv, resolve);
    if (verbRow) return verbRow;
  }
  if (action === "channel") {
    return { lead: inv.path.slice(1).join(" "), object: inv.positionals.find((value) => value.trim()) };
  }
  const lead = action ? LEAD[action] ?? `plugin ${action}` : "plugin";
  if (action === "update-system") return { lead, object: "system plugins" };
  const target = inv.positionals.find((value) => value.trim());
  if (target) return { lead, object: isPath(target) ? folderName(target) : pluginLabel(target, resolve) };
  const dir = inv.flags["from-dir"];
  if (typeof dir === "string" && dir.trim()) return { lead, object: folderName(dir.trim()) };
  if (action === "update") return { lead, object: "all plugins" };
  return { lead };
}
