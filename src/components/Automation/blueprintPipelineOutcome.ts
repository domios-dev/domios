import type { BlueprintJournalNode } from "../../lib/blueprintApi";
import type { PipelineView } from "./pipelineModel";

export function blueprintJournalNodes(pipeline: PipelineView): readonly BlueprintJournalNode[] {
  if (pipeline.nested_run?.id !== pipeline.runId) return [];
  return (pipeline.nested_run?.nodes ?? []).map((row) => {
    if (row.parent_step_index != null || row.step_index == null || row.step_index < 0
      || (row.run_id != null && row.run_id !== pipeline.runId)
      || (pipeline.graph_definition_version != null && row.definition_version !== pipeline.graph_definition_version)) return row;
    const index = pipeline.call_graph?.steps.findIndex((node) => node.id === row.step_name) ?? -1;
    if (index < 0) return row;
    const step = pipeline.nodes.find((node) => node.key === `step-${index}` && node.kind === "step");
    return step?.state === "failed" ? { ...row, state: "failed", reason: step.reason ?? row.reason,
      result_summary: step.resultSummary ?? row.result_summary } : row;
  });
}

function newest(rows: readonly BlueprintJournalNode[]): BlueprintJournalNode | null {
  let latest: BlueprintJournalNode | null = null;
  for (const row of rows) {
    if (!latest || (row.entry_id ?? -Infinity) > (latest.entry_id ?? -Infinity)) latest = row;
  }
  return latest;
}
export function blueprintPipelineOutcome(pipeline: PipelineView): BlueprintJournalNode | null {
  if (!["failed", "held"].includes(pipeline.state) || pipeline.nested_run?.id !== pipeline.runId) return null;
  const rows = blueprintJournalNodes(pipeline).filter((row) => ["failed", "held"].includes(row.state)
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
const HELD_REASONS: Readonly<Record<string, string>> = {
  plan_limit_locked: "the owning workspace, Team or tab is locked by your plan",
  paused: "paused by the owner",
  uncertain_pipeline_claim: "a step claim could not be confirmed after a restart",
  uncertain_pipeline_effect: "a step's effect could not be confirmed after a restart",
  uncertain_pipeline_result: "the result delivery could not be confirmed",
  journal_write_failed: "the run journal could not be written",
  plugin_step_waiting_for_owner_ok: "waiting for your OK",
  plugin_step_queued: "queued",
  plugin_step_running: "generating",
  plugin_step_submitting: "starting",
  plugin_step_unknown: "status unknown; check the existing operation",
  plugin_step_canceling: "canceling; status unknown until confirmed",
  plugin_step_stopped: "observation stopped; prior outputs retained",
  plugin_step_scope_removed: "the operation owner scope is unavailable",
  plugin_step_observation_expired: "the bounded observation period ended",
  plugin_step_unavailable: "the declared plugin step is unavailable",
  plugin_step_input_unavailable: "the step's original input is unavailable",
  plugin_step_store_unavailable: "the operation ledger is unavailable",
  plugin_step_media_unavailable: "run media is unavailable",
  plugin_step_authorization_unavailable: "the original spending authorization is unavailable",
};
export function heldReasonLabel(reason: string): string {
  return HELD_REASONS[reason] ?? reason;
}
