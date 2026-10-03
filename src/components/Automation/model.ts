import type { LaidOutNode } from "../Canvas/layout";
import { automationKey, sameScope, type AutomationGraph, type AutomationNode, type AutomationScope } from "./types";

export function scopedAutomationGraph(graph: AutomationGraph, scope: AutomationScope): AutomationGraph {
  if (!sameScope(graph, scope)) throw new Error("Automation scope mismatch");
  const nodes = graph.nodes.filter((node) => sameScope(node.identity, scope));
  const byId = new Map(nodes.map((node) => [automationKey(node.identity), node]));
  const edges = graph.edges.filter((edge) => sameScope(edge.source, scope) && sameScope(edge.target, scope) && byId.has(automationKey(edge.source)));
  for (const edge of edges) {
    const key = automationKey(edge.target);
    if (byId.has(key)) continue;
    const missing: AutomationNode = {
      identity: edge.target, name: edge.target.id, purpose: null, state: null,
      availability: "missing", hidden: null, schedule: null, cadence_ms: null,
      expires_at: null, outcomes: [], last_result: null, stop: { allowed: false, reason: null },
    };
    byId.set(key, missing);
    nodes.push(missing);
  }
  return { ...graph, nodes, edges };
}

export function automationState(node: AutomationNode): string {
  return node.availability === "available" ? node.state ?? "Unknown" : node.availability;
}

export function layoutAutomationGraph(graph: AutomationGraph) {
  const columns = ["sentinel", "cron", "worker"];
  const rows = [0, 0, 0];
  const nodes: LaidOutNode[] = graph.nodes.map((node) => {
    const column = columns.indexOf(node.identity.kind);
    return {
      id: automationKey(node.identity), kind: node.identity.kind, planId: "", iteration: null, manual: false,
      box: { x: 32 + column * 420, y: 56 + rows[column]++ * 160, width: 260, height: 104 },
    };
  });
  return {
    nodes, byId: new Map(nodes.map((node) => [node.id, node])),
    bounds: { x: 0, y: 0, width: 1160, height: Math.max(260, ...rows.map((row) => 80 + row * 160)) },
  };
}
