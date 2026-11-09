import { useMemo } from "react";
import { sentinelStateLabel } from "./jobRowModel";
import { Network, Workflow } from "lucide-react";
import { usePlanPeerContext } from "../Canvas/usePlanPeerContext";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { blueprintScenarioJobs, blueprintTitle } from "../Automation/scenarios";
import type { AutomationGraphAdapter, AutomationScope } from "../Automation/types";
import { useAttachedAutomationSnapshot } from "../Automation/useAutomationGraph";
import { TeamUnit, type TeamUnitProps } from "../Sidebar/TeamUnit";
import { blueprintJobs, isJobRunning, jobStateLabel, mergeJobs, type MergedJob } from "./jobsModel";
import { requestOpenJob } from "./openJobEvent";

interface SentinelSource {
  sentinelScope?: AutomationScope;
  onOpenSentinel?: (scenarioId: string) => void;
  sentinelAdapter?: AutomationGraphAdapter;
}

const NO_JOBS: readonly MergedJob[] = [];

function jobRow(entry: MergedJob, scope: AutomationScope, onOpenSentinel: (scenarioId: string) => void) {
  if (entry.kind === "blueprint") {
    const blueprint = entry.blueprint.pipeline;
    return <div key={`blueprint:${blueprint.id}`} className="sidebar-team-cron-row">
      <button type="button" className="sidebar-team-cron-main" data-testid="sidebar-blueprint-job" aria-label={`Blueprint ${blueprint.name}, ${jobStateLabel(blueprint.state)}`} title={blueprint.name} onClick={() => requestOpenJob({ workspaceId: scope.workspace_id, target: { teamId: scope.team_id, blueprintId: blueprint.id } })}>
        <Workflow size={11} aria-hidden="true" />
        <span className="sidebar-team-cron-name">{blueprint.name}</span>
        <span className="sidebar-team-cron-state">{jobStateLabel(blueprint.state)}</span>
      </button>
      <span className="w-7 shrink-0" aria-hidden="true" />
    </div>;
  }
  if (entry.kind !== "sentinel") return null;
  const job = entry.sentinel;
  return <div key={`scenario:${job.scenario.id}`} className="sidebar-team-cron-row">
    <button type="button" className="sidebar-team-cron-main" data-testid="sidebar-sentinel-job" aria-label={`${blueprintTitle(job.scenario, job.root)}, ${job.type}, ${sentinelStateLabel(job)}`} title={job.root?.purpose ?? job.type} onClick={() => onOpenSentinel(job.scenario.id)}>
      <Network size={11} aria-hidden="true" />
      <span className="sidebar-team-cron-name">{blueprintTitle(job.scenario, job.root)}</span>
      <span className="sidebar-team-cron-state">{sentinelStateLabel(job)}</span>
    </button>
    <span className="w-7 shrink-0" aria-hidden="true" />
  </div>;
}

/** Team card whose Jobs disclosure lists its Cron rows plus stored agent-authored Blueprint scenarios of the same exact scope. */
export function TeamJobsUnit({ sentinelScope, onOpenSentinel, sentinelAdapter = hostAutomationAdapter, ...props }: TeamUnitProps & SentinelSource) {
  const context = usePlanPeerContext();
  const workspace_id = context.ready && onOpenSentinel ? sentinelScope?.workspace_id : undefined;
  const team_id = sentinelScope?.team_id ?? null;
  // One element shape with or without a scope: a Team registered after its rows must not remount them.
  const scope = useMemo(() => workspace_id ? { workspace_id, team_id } : null, [workspace_id, team_id]);
  const attachment = useMemo(() => scope && { ...scope, connection_id: "sidebar" }, [scope]);
  const state = useAttachedAutomationSnapshot(sentinelAdapter, attachment);
  const jobs = useMemo(() => state.graph && scope ? mergeJobs(
    { workspaceId: scope.workspace_id, teamId: scope.team_id }, [],
    blueprintScenarioJobs(state.graph, scope), blueprintJobs(state.graph),
  ) : NO_JOBS, [state.graph, scope]);
  const unavailable = state.failed && !state.graph;
  const rows = scope && onOpenSentinel ? jobs.map((entry) => jobRow(entry, scope, onOpenSentinel)) : [];
  if (unavailable) rows.push(<div key="sentinels-unavailable" className="sidebar-team-cron-row"><span className="sidebar-team-cron-main cursor-default" data-testid="sidebar-sentinels-unavailable"><Network size={11} aria-hidden="true" /><span className="sidebar-team-cron-name">Blueprints unavailable</span></span></div>);
  return <TeamUnit {...props} jobsUnavailable={unavailable} crons={<>{props.crons}{rows}</>} cronCount={(props.cronCount ?? 0) + jobs.length} workingCronCount={(props.workingCronCount ?? 0) + jobs.filter(isJobRunning).length} />;
}
