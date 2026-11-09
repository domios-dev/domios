import { assertAutomationScope, automationState, isScenarioRoot } from "./model";
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
  const nodes = new Map(graph.nodes.map((node) => [automationKey(node.identity), node]));
  return graph.scenarios.map((scenario) => {
    const team_id = scope.team_id === ALL_JOBS_SCOPE ? scenario.root.team_id : scope.team_id;
    assertAutomationScope(
      graph.all_teams ? { workspace_id: graph.workspace_id, all_teams: false, team_id: scenario.root.team_id } : graph,
      { workspace_id: scope.workspace_id, team_id },
    );
    const node = scenario.root.workspace_id === scope.workspace_id && scenario.root.team_id === team_id ? nodes.get(automationKey(scenario.root)) : undefined;
    const root = node && isScenarioRoot(node, scenario.root) ? node : null;
    return { scenario, type: "Blueprint" as const, root, state: root ? automationState(root) : "Unavailable" };
  });
}

export function blueprintTitle(scenario: AutomationScenario, root: AutomationNode | null): string {
  if (scenario.name.trim() && scenario.name !== scenario.id) return scenario.name;
  return root?.purpose?.trim() || root?.outcomes[0]?.bucket?.trim() || "Untitled blueprint";
}
