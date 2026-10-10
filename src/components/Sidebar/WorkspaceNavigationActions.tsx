import { useMemo } from "react";
import { Map as MapIcon, Workflow } from "lucide-react";
import { blueprintJobs, jobsAccessibleName, jobsIndicator, type JobsFilter, type JobsIndicator } from "../Jobs/jobsModel";
import { jobsLayout, type JobsWorkspace } from "../Jobs/jobsLayout";
import { useCronJobs } from "../Jobs/useCronJobs";
import { useWorkspaceGraphs } from "../Jobs/useWorkspaceGraphs";
import { blueprintScenarioJobs } from "../Automation/scenarios";
import { hostAutomationAdapter } from "../Automation/hostAdapter";

interface WorkspaceNavigationActionsProps {
  workspaceId: string;
  workspaceName: string;
  onOpenMap?: (workspaceId: string) => void;
  onOpenJobs?: (workspaceId: string) => void;
  jobs?: JobsIndicator;
  touchMode?: boolean;
}

const NO_JOBS: JobsIndicator = { count: 0, running: 0 };
const NO_WORKSPACES: readonly JobsWorkspace[] = [];
const SIDEBAR_ACTION = "inline-flex min-w-0 items-center justify-center gap-1.5 rounded px-2 text-xs text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--ct-accent)]";

function Actions({ workspaceId, workspaceName, onOpenMap, onOpenJobs, jobs = NO_JOBS, touchMode = false }: WorkspaceNavigationActionsProps) {
  const size = touchMode ? 16 : 13;
  const buttonSize = touchMode ? "h-10" : "h-7";
  return <div data-testid="workspace-navigation-actions" className="flex min-h-8 items-center gap-1 px-3 py-0.5">
    {onOpenMap && <button type="button" aria-label="Map" title={`Open essence map for ${workspaceName}`} className={`${SIDEBAR_ACTION} ${buttonSize}`} onClick={() => onOpenMap(workspaceId)}>
      <MapIcon size={size} strokeWidth={1.6} aria-hidden="true" /><span>Map</span>
    </button>}
    {onOpenJobs && <button type="button" aria-label={jobsAccessibleName(jobs)} title={`Open jobs for ${workspaceName}`} className={`${SIDEBAR_ACTION} ${buttonSize}`} onClick={() => onOpenJobs(workspaceId)}>
      <Workflow size={size} strokeWidth={1.6} aria-hidden="true" /><span>Jobs</span>
      {jobs.count > 0 && <span aria-hidden="true" className={`tabular-nums ${jobs.running > 0 ? "text-[var(--ct-accent)]" : "text-[var(--ct-muted)]"}`}>{jobs.count}</span>}
    </button>}
  </div>;
}

export function WorkspaceNavigationActions(props: WorkspaceNavigationActionsProps) {
  return <Actions {...props} />;
}

interface LiveWorkspaceNavigationActionsProps extends Omit<WorkspaceNavigationActionsProps, "jobs"> {
  scope: JobsFilter;
  workspaces?: readonly JobsWorkspace[];
}

/** Reads jobs through the Jobs view's own sources and filter, so the count matches what the view lists. */
export function LiveWorkspaceNavigationActions({ scope: scopeProp, workspaces = NO_WORKSPACES, ...props }: LiveWorkspaceNavigationActionsProps) {
  const scope = useMemo<JobsFilter>(() => ({ workspaceId: scopeProp.workspaceId, teamId: scopeProp.teamId }), [scopeProp.workspaceId, scopeProp.teamId]);
  const { sources, cronWorkspace } = useMemo(() => jobsLayout(scope, workspaces, []), [scope, workspaces]);
  const graphs = useWorkspaceGraphs(hostAutomationAdapter, "sidebar", sources);
  const crons = useCronJobs(cronWorkspace);
  const jobs = useMemo(() => {
    if (crons.jobs === null || graphs.some((graph) => graph.graph === null)) return NO_JOBS;
    const sentinels = graphs.flatMap((graph) => graph.graph ? blueprintScenarioJobs(graph.graph, graph.attachment) : []);
    const blueprints = graphs.flatMap((graph) => graph.graph ? blueprintJobs(graph.graph) : []);
    return jobsIndicator({ crons: crons.jobs, sentinels, blueprints }, scope);
  }, [crons.jobs, graphs, scope]);
  return <Actions {...props} jobs={jobs} />;
}
