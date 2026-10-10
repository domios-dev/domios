import { ALL_JOBS_SCOPE, automationKey, sameScope, type AutomationGraph, type AutomationIdentity, type AutomationNode, type AutomationScope } from "./types";

export function isScenarioRoot(node: AutomationNode, root: AutomationIdentity): boolean {
  return node.identity.kind === "blueprint" && automationKey(node.identity) === automationKey(root);
}

export function scopedAutomationGraph(graph: AutomationGraph, scope: AutomationScope): AutomationGraph {
  const allTeams = scope.team_id === ALL_JOBS_SCOPE;
  if (graph.workspace_id !== scope.workspace_id || (allTeams ? graph.all_teams !== true : !sameScope(graph, scope))) throw new Error("Automation scope mismatch");
  const inScope = (identity: AutomationIdentity) => identity.workspace_id === scope.workspace_id && (allTeams || identity.team_id === scope.team_id);
  const scoped = graph.nodes.filter((node) => inScope(node.identity));
  const byId = new Map(scoped.map((node) => [automationKey(node.identity), node]));
  const eligibleEdges = graph.edges.filter((edge) => inScope(edge.source) && inScope(edge.target) && byId.has(automationKey(edge.source)));
  const outgoing = new Map<string, typeof eligibleEdges>();
  for (const edge of eligibleEdges) {
    const key = automationKey(edge.source);
    const entries = outgoing.get(key) ?? [];
    entries.push(edge);
    outgoing.set(key, entries);
  }
  const scenarios = graph.scenarios.filter((scenario) => inScope(scenario.root));
  const roots = scenarios.flatMap((scenario) => {
    const node = byId.get(automationKey(scenario.root));
    return node && isScenarioRoot(node, scenario.root) ? [node] : [];
  });
  const nodes = Array.from(new Map(roots.map((node) => [automationKey(node.identity), node])).values());
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
  const blueprints = graph.blueprints?.filter((blueprint) => {
    const ownerTeam = blueprint.owner?.kind === "team" ? blueprint.owner.team_id : blueprint.owner?.team_id ?? null;
    return allTeams || ownerTeam === scope.team_id;
  });
  const runs = graph.runs?.filter((run) => roots.some((root) => root.identity.id === run.root_node));
  return { ...graph, scenarios, nodes, edges, ...(runs ? { runs } : {}), ...(blueprints ? { blueprints } : {}) };
}

export function automationState(node: AutomationNode): string {
  return node.availability === "available" ? node.state ?? "Unknown" : node.availability;
}
