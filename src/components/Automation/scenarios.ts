import { automationState, scopedAutomationGraph } from "./model";
import { ALL_JOBS_SCOPE, automationKey, type AutomationGraph, type AutomationNode, type AutomationScenario, type AutomationScope } from "./types";

export type BlueprintJobType = "Blueprint";

export interface BlueprintScenarioJob {
  scenario: AutomationScenario;
  type: BlueprintJobType;
  root: AutomationNode | null;
  state: string;
}

/** One entry per stored scenario; its internal watcher stages stay inside that scenario's graph. */
export function blueprintScenarioJobs(graph: AutomationGraph, scope: AutomationScope): BlueprintScenarioJob[] {
  return graph.scenarios.map((scenario) => {
    const scenarioScope = scope.team_id === ALL_JOBS_SCOPE
      ? { workspace_id: scope.workspace_id, team_id: scenario.root.team_id }
      : scope;
    const scenarioGraph = graph.all_teams
      ? { ...graph, all_teams: false, team_id: scenario.root.team_id }
      : graph;
    const participants = scopedAutomationGraph({ ...scenarioGraph, scenarios: [scenario] }, scenarioScope).nodes;
    const root = participants.find((node) => automationKey(node.identity) === automationKey(scenario.root)) ?? null;
    return { scenario, type: "Blueprint" as const, root, state: root ? automationState(root) : "Unavailable" };
  });
}

export function blueprintTitle(scenario: AutomationScenario, root: AutomationNode | null): string {
  if (scenario.name.trim() && scenario.name !== scenario.id) return scenario.name;
  return root?.purpose?.trim() || root?.outcomes[0]?.bucket?.trim() || "Untitled blueprint";
}
