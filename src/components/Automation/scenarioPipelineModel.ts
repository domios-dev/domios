import { graphRuns } from "./blueprintRunModel";
import { scopedAutomationGraph } from "./model";
import { pipelineModel, type PipelineView } from "./pipelineModel";
import type { AutomationGraph, AutomationScenario, BlueprintSnapshot } from "./types";

export function scenarioPipelineModel(graph: AutomationGraph, scenario: AutomationScenario, blueprint: BlueprintSnapshot): PipelineView {
  const pipeline = pipelineModel(blueprint);
  if (blueprint.call_graph?.kind !== "watch" || pipeline.runs?.length) return pipeline;
  const scoped = scopedAutomationGraph({ ...graph, scenarios: [scenario] }, graph);
  const runs = graphRuns(scoped).map((run) => ({
    id: run.key, at: run.at, state: run.state,
    pipeline: { ...pipeline, runs: undefined, runId: run.key, recordedRun: run } as PipelineView,
  }));
  return { ...pipeline, runs, recordedRun: runs[0]?.pipeline.recordedRun, runId: runs[0]?.id ?? pipeline.runId };
}
