import type { BlueprintDocument, BlueprintIrNode, BlueprintValue } from "../../../lib/blueprintApi";
import type { AutomationNode } from "../types";

export interface BlueprintTriggerRuntime {
  nextAt: number | null;
  lastAt: number | null;
  expiresAt: number | null;
}

export interface BlueprintNodeTrigger {
  badge: string;
  label: string;
  cadence: string | null;
  timeout: string | null;
  runtime: BlueprintTriggerRuntime | null;
}

function valueText(value: unknown): string | null {
  if (typeof value === "string" || typeof value === "number") return String(value);
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const fields = value as Readonly<Record<string, unknown>>;
  if ("lit" in fields) return valueText(fields.lit);
  if (typeof fields.input === "string") return `input: ${fields.input}`;
  if (typeof fields.bind === "string") return `from ${fields.bind}`;
  return null;
}

export function triggerRuntime(node: AutomationNode | undefined): BlueprintTriggerRuntime | null {
  return node ? { nextAt: node.detail?.next_check_at_ms ?? null, lastAt: node.detail?.last_probe_at ?? null, expiresAt: node.expires_at } : null;
}

export function nodeTrigger(definition: BlueprintIrNode | BlueprintValue, emitter: boolean, runtime: BlueprintTriggerRuntime | null = null): BlueprintNodeTrigger | undefined {
  if (!definition || typeof definition !== "object" || Array.isArray(definition)) return undefined;
  const fields = definition as Readonly<Record<string, unknown>>;
  const cadence = valueText(fields.every);
  if (!emitter && fields.every === undefined) return undefined;
  const kind = valueText(fields.kind);
  const type = valueText(fields.type) ?? valueText(fields.uses);
  const badge = cadence ? `every ${cadence}` : fields.every !== undefined ? "recurring" : type === "schedule" ? "scheduled" : kind ? `on ${kind}` : type === "input" ? "on input" : type === "webhook" ? "on webhook" : "on event";
  return { badge, label: `Trigger: ${badge}`, cadence, timeout: valueText(fields.timeout), runtime };
}

export function singleCadence(document: BlueprintDocument | null | undefined): string | null {
  if (!document) return null;
  const sources = [document.trigger ? nodeTrigger(document.trigger, true) : undefined, ...[...document.steps, ...(document.other ?? [])].map((node, index) => nodeTrigger(node, document.kind === "watch" && index === 0))].filter((trigger) => trigger !== undefined);
  return sources.length === 1 && sources[0].cadence ? sources[0].badge : null;
}
