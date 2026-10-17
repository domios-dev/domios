import { blueprintScenarioJobs } from "../Automation/scenarios";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { blueprintJobs, mergeJobs } from "../Jobs/jobsModel";
import { jobsLayout, type JobsWorkspace } from "../Jobs/jobsLayout";
import { useCronJobs } from "../Jobs/useCronJobs";
import { useWorkspaceGraphs } from "../Jobs/useWorkspaceGraphs";

export function useGeneralJobs(workspaces: readonly JobsWorkspace[], workspaceId: string) {
  const filter = { workspaceId, teamId: null };
  const { sources } = jobsLayout(filter, workspaces, []);
  const graphs = useWorkspaceGraphs(hostAutomationAdapter, "sidebar", sources);
  const crons = useCronJobs(workspaceId);
  const sentinels = graphs.flatMap((entry) => entry.graph ? blueprintScenarioJobs(entry.graph, entry.attachment) : []);
  const blueprints = graphs.flatMap((entry) => entry.graph ? blueprintJobs(entry.graph) : []);
  return {
    jobs: mergeJobs(filter, crons.jobs ?? [], sentinels, blueprints),
    refresh: crons.refresh,
    failed: crons.failed || graphs.some((entry) => entry.failed),
  };
}
