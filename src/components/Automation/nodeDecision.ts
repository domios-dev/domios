import type { AutomationGraph, AutomationNode, AutomationTransition } from "./types";

export interface NodeKind { ai: boolean; label: string; detail?: string }
export type ClassifierConfig = AutomationGraph["configured_fallback"];

export function aiKind(name: string, model?: string | null): NodeKind {
  const display = name.replace(/^typesafe[-_]/i, "");
  return { ai: true, label: `${display.charAt(0).toUpperCase()}${display.slice(1)}${model ? ` · ${model}` : ""}`, detail: name };
}

export function nodeKind(node: AutomationNode, classifier?: ClassifierConfig): NodeKind {
  if (node.identity.kind === "blueprint") {
    const definition = node.detail?.definition;
    const agent = definition?.decision_kind === "agent" || (definition?.decision_kind === undefined && !!definition?.question && !!definition.buckets?.some((bucket) => !bucket.pattern));
    if (agent) return classifier ? aiKind(classifier.adapter_id, classifier.model) : { ai: true, label: "Agent" };
    if (definition?.buckets?.some((bucket) => bucket.pattern)) return { ai: false, label: "Pattern match" };
  } else if (node.executor?.kind === "agent") return aiKind(node.executor.provider_id ?? "Agent", node.executor.model);
  if (node.detail?.definition?.probe || node.source?.type === "probe") return { ai: false, label: "Script" };
  return { ai: false, label: node.source?.type === "observe" ? "Observer" : node.identity.kind === "cron" ? "Schedule" : "Rule" };
}

export function decisionKind(transition: AutomationTransition): NodeKind | null {
  return transition.decided_by === "pattern" ? { ai: false, label: "Pattern rules" }
    : transition.decided_by === "adapter" ? aiKind(transition.adapter_id ?? "Classifier", transition.model) : null;
}

export function bucketDefinitions(node: AutomationNode) {
  const buckets = node.detail?.definition?.buckets;
  return (buckets?.length ? buckets : node.outcomes.map(({ bucket }) => ({ name: bucket, pattern: null })))
    .map((bucket) => ({ name: bucket.name, description: bucket.pattern ? `Matches pattern: ${bucket.pattern}` : "" }));
}
