import type { LaidOutNode } from "../Canvas/layout";
import { automationKey, sameScope, type AutomationGraph, type AutomationNode, type AutomationScope } from "./types";

function agentSentinel(node: AutomationNode): boolean {
  const definition = node.sentinel_definition;
  return node.identity.kind === "sentinel" && definition?.source === "stored_agent_definition"
    && definition.definition_id === node.identity.id && definition.creator_tab_id === node.identity.owner_tab_id;
}

export function scopedAutomationGraph(graph: AutomationGraph, scope: AutomationScope): AutomationGraph {
  if (!sameScope(graph, scope)) throw new Error("Automation scope mismatch");
  const scoped = graph.nodes.filter((node) => sameScope(node.identity, scope));
  const byId = new Map(scoped.filter((node) => node.identity.kind !== "sentinel" || agentSentinel(node)).map((node) => [automationKey(node.identity), node]));
  const eligibleEdges = graph.edges.filter((edge) => sameScope(edge.source, scope) && sameScope(edge.target, scope) && byId.has(automationKey(edge.source)));
  const outgoing = new Map<string, typeof eligibleEdges>();
  for (const edge of eligibleEdges) {
    const key = automationKey(edge.source);
    const entries = outgoing.get(key) ?? [];
    entries.push(edge);
    outgoing.set(key, entries);
  }
  const nodes = scoped.filter(agentSentinel);
  const reached = new Set(nodes.map((node) => automationKey(node.identity)));
  const edges: typeof eligibleEdges = [];
  for (let index = 0; index < nodes.length; index++) {
    for (const edge of outgoing.get(automationKey(nodes[index].identity)) ?? []) {
      edges.push(edge);
      const key = automationKey(edge.target);
      if (reached.has(key)) continue;
      reached.add(key);
      const target = byId.get(key);
      nodes.push(target ?? {
        identity: edge.target, name: edge.target.id, purpose: null, state: null,
        availability: "missing", hidden: null, schedule: null, cadence_ms: null,
        expires_at: null, outcomes: [], last_result: null, stop: { allowed: false, reason: null },
      });
    }
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
