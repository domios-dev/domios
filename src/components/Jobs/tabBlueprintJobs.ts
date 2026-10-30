import { pipelineModel } from "../Automation/pipelineModel";
import { blueprintScenarioJobs, type BlueprintScenarioJob } from "../Automation/scenarios";
import type { BlueprintSnapshot } from "../Automation/types";
import type { WorkspaceGraph } from "./useWorkspaceGraphs";

type GraphSource = Pick<WorkspaceGraph, "graph" | "attachment">;

export function tabBlueprintJobs(graphs: readonly GraphSource[], tabId: string) {
  const persisted = new Map<string, { workspaceId: string; blueprint: BlueprintSnapshot }>();
  for (const { graph, attachment } of graphs) {
    for (const blueprint of graph?.blueprints ?? []) {
      if (typeof blueprint.id !== "string" || !blueprint.id.trim() || ("dry_run" in blueprint && blueprint.dry_run === true)) continue;
      persisted.set(blueprint.id, { workspaceId: attachment.workspace_id, blueprint });
    }
  }
  const blueprints = [...persisted.values()]
    .filter(({ blueprint }) => blueprint.owner?.tab_id === tabId)
    .map((entry) => ({ ...entry, pipeline: pipelineModel(entry.blueprint) }));
  const owned = new Set(blueprints.map(({ blueprint }) => blueprint.id));
  const sentinels = new Map<string, BlueprintScenarioJob>();
  for (const { graph, attachment } of graphs) {
    if (!graph) continue;
    for (const job of blueprintScenarioJobs(graph, attachment)) {
      if (persisted.get(job.scenario.id)?.workspaceId !== attachment.workspace_id || owned.has(job.scenario.id)) continue;
      sentinels.set(job.scenario.id, job);
    }
  }
  return { blueprints, sentinels: [...sentinels.values()] };
}
