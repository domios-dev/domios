import { useMemo } from "react";
import { DsRenderContext } from "@codeterm/chat-ui";
import type { ApiConfig } from "../../lib/types";
import { PlanViewShell } from "./PlanViewShell";
import type { CanvasWorkspaceTarget } from "./canvasFocusStore";
import { PlanFetchErrorState } from "./planView/PlanFetchErrorState";
import { planSidePanelStore } from "./planSidePanelStore";
import { remotePlanGrantReason, type RemoteTeamPlanTarget } from "./remoteTeamPlanModel";
import { RemotePlanTaskActions } from "./RemotePlanTaskActions";
import { usePlanProjection } from "./usePlanProjection";
import { useRemoteTeamPlan } from "./useRemoteTeamPlan";

export function RemoteTeamPlanCanvas({ api, target, onClose }: {
  api: ApiConfig; target: CanvasWorkspaceTarget & { remoteTeam: RemoteTeamPlanTarget }; onClose: () => void;
}) {
  const { remoteTeam } = target;
  const remote = useRemoteTeamPlan(api, remoteTeam);
  const activePlan = remote.plan?.active_plan;
  const planId = activePlan?.id ?? null;
  const projection = usePlanProjection(remote.peerApi, planId, "summary");
  const plan = projection.plan?.team_id === remoteTeam.teamId ? projection.plan : null;
  const grant = remote.plan?.remote_plan_actions;
  const grantReason = remotePlanGrantReason(grant);
  const error = remote.error ?? projection.error ?? (projection.plan && !plan ? "Plan belongs to a different Team." : null);
  const jump = target.planId && target.planId !== planId ? undefined : target.jump;
  const workspaceTarget = useMemo(() => ({ ...target, planId: planId ?? undefined, jump }), [target, planId, jump]);
  return <DsRenderContext.Provider value={{ authority: remoteTeam.peerId, trusted: false, proseOnly: true }}>
    <div className="flex h-full flex-col" aria-label={`Plan for ${remoteTeam.name}`}>
      {grantReason && remote.plan && <p role="status" className="shrink-0 px-4 py-2 text-xs">Read-only. {grantReason}</p>}
      {error || !plan ? <>
        <button type="button" className="self-end px-4 py-2" onClick={onClose}>Close Plan</button>
        <PlanFetchErrorState error={error} loading={remote.loading || projection.loading} onRetry={remote.error ? remote.retry : projection.retry} />
        {remote.plan && !activePlan && <p className="p-4">This Team has no active Plan.</p>}
      </> : <div className="min-h-0 flex-1">
        <PlanViewShell key={plan.id} mode="workspace" plans={[plan]} planId={plan.id} teamId={remoteTeam.teamId}
          api={remote.peerApi} workspaceTarget={workspaceTarget} onClose={onClose}
          planIndex={[{
            plan_id: plan.id, file_path: "", title: plan.title, status: plan.status,
            owner: plan.owner, team_id: plan.team_id, updated_at: plan.updated_at, assigned_tab_ids: [],
          }]}
          onOpenPlanTarget={({ taskId, planId }) => {
            if (planId === plan.id) planSidePanelStore.open({ planId, taskId, remoteTeam });
          }}
          renderTaskActions={detail => <RemotePlanTaskActions key={detail.task_id} target={remoteTeam} detail={detail} grant={grant} workspaceId={remote.plan!.team.workspace_id} />}
        />
      </div>}
    </div>
  </DsRenderContext.Provider>;
}
