import type { BlueprintJournalNode } from "../../lib/blueprintApi";
import type { PipelineView } from "./pipelineModel";
export function blueprintPipelineOutcome(pipeline: PipelineView): BlueprintJournalNode | null {
  if (!["failed", "held"].includes(pipeline.state) || pipeline.nested_run?.id !== pipeline.runId) return null;
  const rows = pipeline.nested_run?.nodes ?? [];
  const byMarker = new Map(rows.filter((row) => row.step_index != null).map((row) => [row.step_index, row]));
  const depth = (row: BlueprintJournalNode) => {
    let value = 0, parent = row.parent_step_index;
    const seen = new Set<number>();
    while (parent != null && !seen.has(parent)) {
      seen.add(parent);
      const ancestor = byMarker.get(parent);
      if (!ancestor) break;
      value++; parent = ancestor.parent_step_index;
    }
    return value;
  };
  let stop: BlueprintJournalNode | null = null;
  for (const row of rows) {
    if (!["failed", "held"].includes(row.state)) continue;
    if (!stop || depth(row) > depth(stop)) stop = row;
  }
  return stop;
}
export function canStopPipelineRun(pipeline: PipelineView, selected: PipelineView): boolean {
  return pipeline.state === "running" && selected.state === "running" && selected.runId != null && selected.runId === pipeline.runId;
}
