import type { AutomationGraph, AutomationNode, AutomationTransition } from "./types";

export interface NodeKind { ai: boolean; label: string; detail?: string }
export type ClassifierConfig = AutomationGraph["configured_fallback"];

export function aiKind(name: string, model?: string | null): NodeKind {
  const display = name.replace(/^typesafe[-_]/i, "");
  return { ai: true, label: `${display.charAt(0).toUpperCase()}${display.slice(1)}${model ? ` · ${model}` : ""}`, detail: name };
}

export function nodeKind(node: AutomationNode, classifier?: ClassifierConfig): NodeKind {
  if (node.executor?.kind === "agent") return aiKind(node.executor.provider_id ?? "Agent", node.executor.model);
  if (node.detail?.definition?.buckets?.length && node.detail.definition.buckets.every((bucket) => bucket.pattern)) return { ai: false, label: "Pattern match" };
  if (node.identity.kind === "sentinel" && node.detail?.definition?.buckets?.some((bucket) => !bucket.pattern) && classifier) return aiKind(classifier.adapter_id, classifier.model);
  if (node.detail?.definition?.probe || node.source?.type === "probe") return { ai: false, label: "Script" };
  return { ai: false, label: node.source?.type === "observe" ? "Observer" : node.identity.kind === "cron" ? "Schedule" : "Rule" };
}

export function decisionKind(transition: AutomationTransition): NodeKind | null {
  return transition.decided_by === "pattern" ? { ai: false, label: "Pattern rules" }
    : transition.decided_by === "adapter" ? aiKind(transition.adapter_id ?? "Classifier", transition.model) : null;
}

export function routeDefinitions(node: AutomationNode) {
  const definition = node.detail?.definition;
  const routes = definition?.buckets?.length ? definition.buckets : node.outcomes.map(({ bucket }) => ({ name: bucket, pattern: null }));
  const escape = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const names = routes.map((route) => escape(route.name)).join("|");
  const contract = definition?.question ?? "";
  return routes.map((route) => {
    const match = contract.match(new RegExp(`(?:^|[\\s;,{])(?:[\\x22\\x27\\x60*]*)${escape(route.name)}(?:[\\x22\\x27\\x60*]*)\\s*(?::|=|[-–—]|\\bif\\b)\\s*([\\s\\S]*?)(?=(?:[;\\n,]\\s*|\\s+)(?:[\\x22\\x27\\x60*]*)(?:${names})(?:[\\x22\\x27\\x60*]*)\\s*(?::|=|[-–—]|\\bif\\b)|$)`, "i"));
    const description = route.pattern ? `Matches pattern: ${route.pattern}` : match?.[1].replace(/\s+/g, " ").replace(/[;\s]+$/, "").trim() || "No separate route definition in the stored contract.";
    return { name: route.name, description };
  });
}
