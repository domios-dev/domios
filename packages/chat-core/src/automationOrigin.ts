import type { ChatOrigin } from "./types";

type AutomationOrigin = Extract<ChatOrigin, { kind: "automation" }>;

export interface AutomationSource {
  from?: string;
  context?: string;
  label?: string;
  tab?: string;
  model?: string;
  purpose?: string;
}

function contextOrigin(context: string): AutomationOrigin | null {
  if (context.startsWith("cron:")) {
    const rest = context.slice(5);
    const last = rest.lastIndexOf(":");
    const name = last > 0 ? rest.slice(0, last) : rest;
    const detail = last > 0 ? rest.slice(last + 1) : "";
    return name ? { kind: "automation", source: "cron", name, ...(detail ? { detail } : {}) } : null;
  }
  if (context.startsWith("blueprint:")) {
    const rest = context.slice(10);
    const split = rest.indexOf(":");
    const name = split >= 0 ? rest.slice(0, split) : rest;
    const detail = split >= 0 ? rest.slice(split + 1) : "";
    return name ? { kind: "automation", source: "blueprint", name, ...(detail ? { detail } : {}) } : null;
  }
  return null;
}

/** Cron, blueprint, and utility-shell deliveries, from a <domios> frame or a structured chat row. */
export function automationOrigin(source: AutomationSource): AutomationOrigin | null {
  const { from, context, label, tab, model, purpose } = source;
  if (from === "system" && context) return contextOrigin(context);
  if (from === "report" && tab && label?.startsWith("cron:")) {
    return { kind: "automation", source: "cron", name: label.slice(5), tabIdShort: tab, report: true, ...(model ? { model } : {}) };
  }
  if (from === "tab" && tab && purpose) {
    return { kind: "automation", source: "utility", name: label ?? purpose, purpose, tabIdShort: tab };
  }
  return null;
}
