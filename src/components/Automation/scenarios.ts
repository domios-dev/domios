import { automationState, scopedAutomationGraph } from "./model";
import { ALL_JOBS_SCOPE, automationKey, type AutomationGraph, type AutomationNode, type AutomationScenario, type AutomationScope } from "./types";

export type SentinelJobType = "Blueprint";

export interface SentinelJob {
  scenario: AutomationScenario;
  type: SentinelJobType;
  root: AutomationNode | null;
  state: string;
}

export function sentinelJobType(_nodes: readonly AutomationNode[]): SentinelJobType {
  return "Blueprint";
}

/** One entry per stored scenario; its internal watcher stages stay inside that scenario's graph. */
export function sentinelJobs(graph: AutomationGraph, scope: AutomationScope): SentinelJob[] {
  return graph.scenarios.map((scenario) => {
    const scenarioScope = scope.team_id === ALL_JOBS_SCOPE
      ? { workspace_id: scope.workspace_id, team_id: scenario.root.team_id }
      : scope;
    const scenarioGraph = graph.all_teams
      ? { ...graph, all_teams: false, team_id: scenario.root.team_id }
      : graph;
    const participants = scopedAutomationGraph({ ...scenarioGraph, scenarios: [scenario] }, scenarioScope).nodes;
    const root = participants.find((node) => automationKey(node.identity) === automationKey(scenario.root)) ?? null;
    return { scenario, type: sentinelJobType(participants), root, state: root ? automationState(root) : "Unavailable" };
  });
}

export function sentinelTitle(scenario: AutomationScenario, root: AutomationNode | null): string {
  if (scenario.name.trim() && scenario.name !== scenario.id) return scenario.name;
  return root?.purpose?.trim() || root?.outcomes[0]?.bucket?.trim() || scenario.id;
}
