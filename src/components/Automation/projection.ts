import { automationKey, type AutomationEdge, type AutomationGraph, type AutomationHandler, type AutomationHandlerResult, type AutomationNode } from "./types";

/** Declared actions drawn per participant before the rest fold into one counted overflow leaf. */
export const MAX_ACTIONS_PER_PARTICIPANT = 24;

/** A declared handler (or declared empty outcome) that has no participant of its own; it is presentation, never an identity. */
export interface ActionLeaf {
  key: string;
  sourceKey: string;
  source: AutomationNode;
  bucket: string;
  handler: AutomationHandler | null;
  taken: boolean;
  result: AutomationHandlerResult | null;
  overflow: number;
}

export type PresentationNode =
  | { key: string; participant: AutomationNode; action?: undefined }
  | { key: string; action: ActionLeaf; participant?: undefined };

export interface PresentationEdge {
  id: string;
  from: string;
  to: string;
  bucket: string;
  handler_id: string | null;
  handler_type: string | null;
  status: string | null;
  evidence: "configured" | "observed" | "declared";
  taken: boolean;
  source: AutomationEdge | null;
}

export interface PresentationGraph {
  nodes: PresentationNode[];
  edges: PresentationEdge[];
}

function inLastAccepted(node: AutomationNode, bucket: string, handler: AutomationHandler) {
  return node.last_result?.bucket === bucket ? node.last_result.handlers.find((entry) => entry.handler_id === handler.id) : undefined;
}

/** Adds every declared outcome handler not already drawn by an exact graph edge, keyed by source identity, bucket and handler. */
export function projectAutomationGraph(graph: AutomationGraph): PresentationGraph {
  const nodes: PresentationNode[] = graph.nodes.map((participant) => ({ key: automationKey(participant.identity), participant }));
  const known = new Set(nodes.map((node) => node.key));
  const routeKey = (source: string, bucket: string, handler: string) => JSON.stringify([source, bucket, handler]);
  const drawn = new Set(graph.edges.map((edge) => routeKey(automationKey(edge.source), edge.bucket, edge.handler_id)));
  const edges: PresentationEdge[] = graph.edges.map((edge) => ({
    id: edge.id, from: automationKey(edge.source), to: automationKey(edge.target), bucket: edge.bucket,
    handler_id: edge.handler_id, handler_type: edge.handler_type, status: edge.result?.status ?? null,
    evidence: edge.evidence.kind, taken: edge.evidence.kind === "observed", source: edge,
  }));
  const used = new Set<string>();
  const unique = (parts: readonly unknown[]) => {
    let key = JSON.stringify(parts);
    for (let repeat = 1; used.has(key); repeat++) key = JSON.stringify([...parts, repeat]);
    used.add(key);
    return key;
  };

  for (const node of graph.nodes) {
    const sourceKey = automationKey(node.identity);
    let drawnActions = 0;
    let overflow = 0;
    const leaf = (bucket: string, handler: AutomationHandler | null) => {
      const accepted = handler ? inLastAccepted(node, bucket, handler) : undefined;
      const result = accepted ?? handler?.last_result ?? null;
      const status = result?.status ?? null;
      const id = unique(["handler", sourceKey, bucket, handler?.id ?? null]);
      const target = handler?.target && known.has(automationKey(handler.target)) ? automationKey(handler.target) : null;
      const to = target ?? id;
      if (!target) nodes.push({ key: id, action: { key: id, sourceKey, source: node, bucket, handler, taken: accepted !== undefined, result, overflow: 0 } });
      edges.push({ id, from: sourceKey, to, bucket, handler_id: handler?.id ?? null, handler_type: handler?.type ?? null, status, evidence: "declared", taken: accepted !== undefined, source: null });
    };
    for (const outcome of node.outcomes) {
      const pending = outcome.handlers.length ? outcome.handlers.filter((handler) => !drawn.has(routeKey(sourceKey, outcome.bucket, handler.id))) : [null];
      for (const handler of pending) {
        if (drawnActions >= MAX_ACTIONS_PER_PARTICIPANT) { overflow++; continue; }
        drawnActions++;
        leaf(outcome.bucket, handler);
      }
    }
    if (overflow) {
      const id = unique(["overflow", sourceKey]);
      nodes.push({ key: id, action: { key: id, sourceKey, source: node, bucket: "", handler: null, taken: false, result: null, overflow } });
      edges.push({ id, from: sourceKey, to: id, bucket: "", handler_id: null, handler_type: null, status: null, evidence: "declared", taken: false, source: null });
    }
  }
  return { nodes, edges };
}
