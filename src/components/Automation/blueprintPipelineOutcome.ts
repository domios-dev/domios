import type { BlueprintJournalNode } from "../../lib/blueprintApi";
import type { PipelineView } from "./pipelineModel";

function newest(rows: readonly BlueprintJournalNode[]): BlueprintJournalNode | null {
  let latest: BlueprintJournalNode | null = null;
  for (const row of rows) {
    if (!latest || (row.entry_id ?? -Infinity) > (latest.entry_id ?? -Infinity)) latest = row;
  }
  return latest;
}
export function blueprintPipelineOutcome(pipeline: PipelineView): BlueprintJournalNode | null {
  if (!["failed", "held"].includes(pipeline.state) || pipeline.nested_run?.id !== pipeline.runId) return null;
  const rows = (pipeline.nested_run?.nodes ?? []).filter((row) => ["failed", "held"].includes(row.state)
    && (row.run_id == null || row.run_id === pipeline.runId));
  let stop = newest(rows.filter((row) => row.parent_step_index == null && row.step_name !== "$run"));
  const seen = new Set<number>();
  while (stop?.step_index != null) {
    if (seen.has(stop.step_index)) return null;
    seen.add(stop.step_index);
    const child = newest(rows.filter((row) => row.parent_step_index === stop!.step_index));
    if (!child) break;
    stop = child;
  }
  return stop;
}
export function canStopPipelineRun(pipeline: PipelineView, selected: PipelineView): boolean {
  return pipeline.state === "running" && selected.state === "running" && selected.runId != null && selected.runId === pipeline.runId;
}
