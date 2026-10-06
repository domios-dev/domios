import { Network, Radar, Workflow } from "lucide-react";
import { usePlanPeerContext } from "../Canvas/usePlanPeerContext";
import { hostAutomationAdapter } from "../Automation/hostAdapter";
import { sentinelJobs, sentinelTitle } from "../Automation/scenarios";
import type { AutomationGraphAdapter, AutomationScope } from "../Automation/types";
import { useAutomationGraph } from "../Automation/useAutomationGraph";
import { TeamUnit, type TeamUnitProps } from "../Sidebar/TeamUnit";
import { requestOpenJob } from "./openJobEvent";

interface SentinelSource {
  sentinelScope?: AutomationScope;
  onOpenSentinel?: (scenarioId: string) => void;
  sentinelAdapter?: AutomationGraphAdapter;
}

function TeamUnitWithSentinels({ scope, generation, onOpenSentinel, adapter, ...props }: TeamUnitProps & { scope: AutomationScope; generation: number; onOpenSentinel: (scenarioId: string) => void; adapter: AutomationGraphAdapter }) {
  const state = useAutomationGraph(adapter, { ...scope, connection_id: `host:${generation}` });
  const jobs = state.graph ? sentinelJobs(state.graph, scope) : [];
  const rows = jobs.map((job) => {
    const Icon = job.type === "Sentinel graph" ? Network : Radar;
    return <div key={`scenario:${job.scenario.id}`} className="sidebar-team-cron-row">
      <button type="button" className="sidebar-team-cron-main" data-testid="sidebar-sentinel-job" aria-label={`${sentinelTitle(job.scenario, job.root)}, ${job.type}, ${job.state}`} title={job.root?.purpose ?? job.type} onClick={() => onOpenSentinel(job.scenario.id)}>
        <Icon size={11} aria-hidden="true" />
        <span className="sidebar-team-cron-name">{sentinelTitle(job.scenario, job.root)}</span>
        <span className="sidebar-team-cron-state">{job.type === "Sentinel graph" ? "graph · " : ""}{job.state}</span>
      </button>
      <span className="w-7 shrink-0" aria-hidden="true" />
    </div>;
  });
  for (const blueprint of state.graph?.blueprints ?? []) {
    if (blueprint.owner?.kind !== "team" || blueprint.owner.team_id !== scope.team_id) continue;
    rows.push(<div key={`blueprint:${blueprint.id}`} className="sidebar-team-cron-row">
      <button type="button" className="sidebar-team-cron-main" data-testid="sidebar-blueprint-job" aria-label={`Blueprint ${blueprint.name}, ${blueprint.state}`} title={blueprint.name} onClick={() => requestOpenJob({ workspaceId: scope.workspace_id, target: { teamId: scope.team_id, blueprintId: blueprint.id } })}>
        <Workflow size={11} aria-hidden="true" />
        <span className="sidebar-team-cron-name">{blueprint.name}</span>
        <span className="sidebar-team-cron-state">{blueprint.state}</span>
      </button>
      <span className="w-7 shrink-0" aria-hidden="true" />
    </div>);
  }
  if (state.failed && !state.graph) rows.push(<div key="sentinels-unavailable" className="sidebar-team-cron-row"><span className="sidebar-team-cron-main cursor-default" data-testid="sidebar-sentinels-unavailable"><Radar size={11} aria-hidden="true" /><span className="sidebar-team-cron-name">Sentinels unavailable</span></span></div>);
  return <TeamUnit {...props} crons={<>{props.crons}{rows}</>} cronCount={(props.cronCount ?? 0) + rows.length} />;
}

/** Team card whose Jobs disclosure lists its Cron rows plus stored agent-authored Sentinel scenarios of the same exact scope. */
export function TeamJobsUnit({ sentinelScope, onOpenSentinel, sentinelAdapter = hostAutomationAdapter, ...props }: TeamUnitProps & SentinelSource) {
  const context = usePlanPeerContext();
  if (!sentinelScope || !onOpenSentinel || !context.ready) return <TeamUnit {...props} />;
  return <TeamUnitWithSentinels {...props} scope={sentinelScope} generation={context.generation} onOpenSentinel={onOpenSentinel} adapter={sentinelAdapter} />;
}
