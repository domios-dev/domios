import { blueprintScenarioJobs } from "../Automation/scenarios";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { blueprintJobs, mergeJobs, NO_JOBS_FILTER } from "../Jobs/jobsModel";
import { jobsLayout, type JobsWorkspace } from "../Jobs/jobsLayout";
import { useCronJobs } from "../Jobs/useCronJobs";
import { useWorkspaceGraphs } from "../Jobs/useWorkspaceGraphs";

export function useGeneralJobs(workspaces: readonly JobsWorkspace[]) {
  const { sources } = jobsLayout(NO_JOBS_FILTER, workspaces, []);
  const graphs = useWorkspaceGraphs(hostAutomationAdapter, "sidebar", sources);
  const crons = useCronJobs(null);
  const sentinels = graphs.flatMap((entry) => entry.graph ? blueprintScenarioJobs(entry.graph, entry.attachment) : []);
  const blueprints = graphs.flatMap((entry) => entry.graph ? blueprintJobs(entry.graph) : []);
  return {
    jobs: mergeJobs(NO_JOBS_FILTER, crons.jobs ?? [], sentinels, blueprints),
    failed: crons.failed || graphs.some((entry) => entry.failed),
  };
}
