import { useCallback, useMemo } from "react";
import type { ReturnTo } from "../../lib/returnTo";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { pipelineModel } from "../Automation/pipelineModel";
import { blueprintScenarioJobs } from "../Automation/scenarios";
import type { AutomationGraphAdapter } from "../Automation/types";
import { hostJobsActionAdapter, jobWorkspace, type JobActionContext, type JobsActionAdapter } from "./jobActions";
import { jobsForTab, mergeJobs, mergedJobId, NO_JOBS_FILTER, tabJobKey, type MergedJob } from "./jobsModel";
import { requestOpenJob } from "./openJobEvent";
import { useCronJobs } from "./useCronJobs";
import { useWorkspaceGraphs } from "./useWorkspaceGraphs";

const NO_SOURCES: readonly string[] = [];

export function openTabJob(job: MergedJob, returnTo?: ReturnTo) {
  requestOpenJob({
    workspaceId: jobWorkspace(job),
    returnTo,
    target: job.kind === "cron" ? { teamId: null, cronId: job.cron.id } : job.kind === "blueprint" ? { teamId: null, blueprintId: job.blueprint.pipeline.id } : { teamId: null, scenarioId: job.sentinel.scenario.id },
  });
}

/** Jobs bound to a tab, read from the same Cron list and Sentinel graph sources as the Jobs view. */
export function useTabJobs(
  tabId: string,
  workspaceId: string | null,
  adapter: AutomationGraphAdapter = hostAutomationAdapter,
  jobActionAdapter: JobsActionAdapter = hostJobsActionAdapter,
) {
  const sources = useMemo(() => workspaceId ? [workspaceId] : NO_SOURCES, [workspaceId]);
  const graphs = useWorkspaceGraphs(adapter, "composer", sources);
  const crons = useCronJobs(null);
  const sentinels = useMemo(() => graphs.flatMap((graph) => graph.graph ? blueprintScenarioJobs(graph.graph, graph.attachment) : []), [graphs]);
  const blueprints = useMemo(() => graphs.flatMap((graph) => (graph.graph?.blueprints ?? [])
    .filter((blueprint) => blueprint.owner?.tab_id === tabId)
    .map((blueprint) => ({ workspaceId: graph.attachment.workspace_id, blueprint, pipeline: pipelineModel(blueprint) }))), [graphs, tabId]);
  const merged = useMemo(() => mergeJobs(NO_JOBS_FILTER, crons.jobs ?? [], sentinels), [crons.jobs, sentinels]);
  const chips = useMemo(() => jobsForTab(merged, tabId), [merged, tabId]);
  const byKey = useMemo(() => new Map(merged.map((job) => [tabJobKey({ kind: job.kind, id: mergedJobId(job) }), job])), [merged]);
  const graphFor = useCallback((id: string) => graphs.find((graph) => graph.attachment.workspace_id === id), [graphs]);
  const context = useMemo<JobActionContext>(() => ({ adapter, jobActions: jobActionAdapter, graphFor, refreshCrons: crons.refresh }), [adapter, jobActionAdapter, graphFor, crons.refresh]);
  const refresh = useCallback(
    () => Promise.allSettled([crons.refresh(), ...graphs.map((graph) => graph.refresh())]),
    [crons, graphs],
  );
  return { chips, byKey, blueprints, context, refresh };
}
