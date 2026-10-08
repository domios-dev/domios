import type { ActionLeaf } from "./projection";
import type { AutomationNode } from "./types";

export function nodeInspector(node: AutomationNode, action: ActionLeaf | null, nodes: readonly AutomationNode[] = []) {
  const definition = node.detail?.definition;
  const handler = action?.handler;
  const command = action && definition?.commands?.find((entry) => entry.bucket === action.bucket && entry.handler_id === handler?.id);
  const properties: [string, string][] = [];
  const kind = action ? handler?.type ?? "outcome" : node.source?.type ?? node.executor?.kind ?? node.identity.kind;
  properties.push(["Kind", kind]);
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
    if (node.outcomes.length) properties.push(["Routes", node.outcomes.map((entry) => entry.bucket).join(", ")]);
  }
  const text = action ? command?.command : definition?.probe;
  return {
    name: action ? handler?.label || handler?.id || action.bucket : node.name,
    purpose: action ? handler?.type === "command" ? "Runs this command when the route is taken." : handler ? `Runs the ${handler.type} action when the route is taken.` : "Ends this route without an action." : node.purpose || "Performs this stage of the Blueprint.",
    properties,
    code: text ? { text, language: definition?.language ?? "bash", truncated: action ? command?.command_truncated : definition?.probe_truncated } : null,
    codeUnavailable: !!definition?.unavailable || (!!action && handler?.type === "command" && !text),
  };
}
