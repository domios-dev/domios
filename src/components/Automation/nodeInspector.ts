import { decisionRule, nodeKind, nodeType, routeDefinitions, type ClassifierConfig } from "./nodeDecision";
import type { ActionLeaf } from "./projection";
import type { AutomationNode } from "./types";

export function nodeInspector(node: AutomationNode, action: ActionLeaf | null, nodes: readonly AutomationNode[] = [], classifier?: ClassifierConfig) {
  const definition = node.detail?.definition;
  const handler = action?.handler;
  const command = action && definition?.commands?.find((entry) => entry.bucket === action.bucket && entry.handler_id === handler?.id);
  const properties: [string, string][] = [];
  const kind = action ? handler?.type ?? "outcome" : node.source?.type ?? node.executor?.kind ?? node.identity.kind;
  const badge = action ? handler?.type === "command" ? { ai: false, label: "Command" } : handler?.target && nodes.some((entry) => entry.identity.id === handler.target?.id && entry.executor?.kind === "agent") ? nodeKind(nodes.find((entry) => entry.identity.id === handler.target?.id)!) : { ai: false, label: kind.charAt(0).toUpperCase() + kind.slice(1) } : nodeKind(node, classifier);
  properties.push(["Type", action ? kind === "command" ? "Code" : badge.ai ? "Agent" : kind.charAt(0).toUpperCase() + kind.slice(1) : nodeType(node, classifier)]);
  if (action) {
    properties.push(["Route", action.bucket]);
    if (handler?.target) properties.push(["Target", nodes.find((entry) => entry.identity.id === handler.target?.id)?.name ?? "Linked node"]);
    if (handler?.timeout_ms) properties.push(["Timeout", `${handler.timeout_ms} ms`]);
  } else {
    if (node.source?.kind) properties.push(["Input", node.source.kind]);
    if (node.source?.display_name) properties.push(["Watches", node.source.display_name]);
    if (node.detail?.observe_target) properties.push(["Watches", nodes.find((entry) => entry.identity.id === node.detail?.observe_target?.id)?.name ?? `Observed ${node.detail.observe_target.kind}`]);
    if (node.executor?.provider_id) properties.push(["Provider", node.executor.provider_id]);
    if (node.executor?.model) properties.push(["Model", node.executor.model]);
    properties.push(["Decides with", decisionRule(node, classifier)]);
  }
  const text = action ? command?.command : definition?.probe;
  return {
    name: action ? handler?.label || handler?.id || action.bucket : node.name,
    purpose: action ? handler?.type === "command" ? "Runs this command when the route is taken." : handler ? `Runs the ${handler.type} action when the route is taken.` : "Ends this route without an action." : definition?.probe ? "Runs the probe and routes its result using the decision rule below." : node.detail?.observe_target ? "Watches the linked node and routes its result." : node.purpose || "Performs the configured node action.",
    properties,
    badge,
    routes: action ? [] : routeDefinitions(node),
    contract: action ? null : definition?.question,
    code: text ? { text, language: definition?.language ?? "bash", truncated: action ? command?.command_truncated : definition?.probe_truncated } : null,
    codeUnavailable: !!definition?.unavailable || (!!action && handler?.type === "command" && !text),
  };
}
