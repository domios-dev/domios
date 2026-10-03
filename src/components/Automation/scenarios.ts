import { automationState, scopedAutomationGraph } from "./model";
import { automationKey, type AutomationGraph, type AutomationNode, type AutomationScenario, type AutomationScope } from "./types";

export type SentinelJobType = "Sentinel" | "Sentinel graph";

export interface SentinelJob {
  scenario: AutomationScenario;
  type: SentinelJobType;
  root: AutomationNode | null;
  state: string;
}

export function sentinelJobType(nodes: readonly AutomationNode[]): SentinelJobType {
  return nodes.filter((node) => node.identity.kind === "sentinel").length > 1 ? "Sentinel graph" : "Sentinel";
}

/** One entry per stored scenario; its internal watcher stages stay inside that scenario's graph. */
export function sentinelJobs(graph: AutomationGraph, scope: AutomationScope): SentinelJob[] {
  return graph.scenarios.map((scenario) => {
    const participants = scopedAutomationGraph({ ...graph, scenarios: [scenario] }, scope).nodes;
    const root = participants.find((node) => automationKey(node.identity) === automationKey(scenario.root)) ?? null;
    return { scenario, type: sentinelJobType(participants), root, state: root ? automationState(root) : "Unavailable" };
  });
}
