import { useMemo } from "react";
import { Map as MapIcon, Workflow } from "lucide-react";
import { useWorkspaceCrons } from "../../hooks/useWorkspaceCrons";
import { ALL_JOBS, activeJobsCount } from "../Jobs/jobsModel";
import { sentinelJobs } from "../Automation/scenarios";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { useAutomationGraph } from "../Automation/useAutomationGraph";
import type { AutomationAttachment } from "../Automation/types";

interface WorkspaceNavigationActionsProps {
  workspaceId: string;
  workspaceName: string;
  onOpenMap?: (workspaceId: string) => void;
  onOpenJobs?: (workspaceId: string) => void;
  activeJobsCount?: number;
  touchMode?: boolean;
}

const SIDEBAR_ACTION = "inline-flex min-w-0 items-center justify-center gap-1.5 rounded px-2 text-xs text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--ct-accent)]";

function Actions({ workspaceId, workspaceName, onOpenMap, onOpenJobs, activeJobsCount, touchMode = false }: WorkspaceNavigationActionsProps) {
  const size = touchMode ? 16 : 13;
  const buttonSize = touchMode ? "h-10" : "h-7";
  return <div data-testid="workspace-navigation-actions" className="flex min-h-8 items-center gap-1 px-3 py-0.5">
    {onOpenMap && <button type="button" aria-label="Map" title={`Open essence map for ${workspaceName}`} className={`${SIDEBAR_ACTION} ${buttonSize}`} onClick={() => onOpenMap(workspaceId)}>
      <MapIcon size={size} strokeWidth={1.6} aria-hidden="true" /><span>Map</span>
    </button>}
    {onOpenJobs && <button type="button" aria-label={activeJobsCount ? `Jobs, ${activeJobsCount} active` : "Jobs"} title={`Open jobs for ${workspaceName}`} className={`${SIDEBAR_ACTION} ${buttonSize}`} onClick={() => onOpenJobs(workspaceId)}>
      <Workflow size={size} strokeWidth={1.6} aria-hidden="true" /><span>Jobs</span>
      {activeJobsCount ? <span aria-hidden="true" className="rounded border border-[var(--ct-border)] px-1 py-px text-[0.625rem] leading-tight">{activeJobsCount}</span> : null}
    </button>}
  </div>;
}

export function WorkspaceNavigationActions(props: WorkspaceNavigationActionsProps) {
  return <Actions {...props} />;
}

export function LiveWorkspaceNavigationActions({ workspaceId, workspaceName, onOpenMap, onOpenJobs, touchMode }: Omit<WorkspaceNavigationActionsProps, "activeJobsCount">) {
  const attachment = useMemo<AutomationAttachment>(() => ({ workspace_id: workspaceId, team_id: ALL_JOBS, connection_id: `sidebar:${workspaceId}` }), [workspaceId]);
  const graph = useAutomationGraph(hostAutomationAdapter, attachment);
  const crons = useWorkspaceCrons(workspaceId);
  const count = useMemo(() => {
    if (!graph.graph) return 0;
    const sentinels = sentinelJobs(graph.graph, attachment);
    return activeJobsCount(workspaceId, crons.jobs, sentinels);
  }, [attachment, crons.jobs, graph.graph, workspaceId]);
  return <Actions workspaceId={workspaceId} workspaceName={workspaceName} onOpenMap={onOpenMap} onOpenJobs={onOpenJobs} activeJobsCount={count} touchMode={touchMode} />;
}
