import { useMemo, type RefObject } from "react";
import { DsRenderContext } from "@codeterm/chat-ui";
import { Maximize2 } from "lucide-react";
import type { ApiConfig } from "../../lib/types";
import { PlanFetchErrorState } from "./planView/PlanFetchErrorState";
import { PlanSidePanel } from "./planView/PlanSidePanel";
import { SidePanelFrame } from "./planView/SidePanelFrame";
import { canonicalTaskStatusLabel } from "./planView/taskStatusLabel";
import { planSidePanelStore, type PlanSidePanelTarget } from "./planSidePanelStore";
import { remotePlanGrantReason, type RemoteTeamPlanTarget } from "./remoteTeamPlanModel";
import { RemotePlanTaskActions } from "./RemotePlanTaskActions";
import { taskDisplayKey } from "./taskDisplayKey";
import { usePlanListWindow } from "./usePlanListWindow";
import { usePlanTaskDetail } from "./usePlanTaskDetail";
import type { PresenceState } from "./usePresence";
import { useRemoteTeamPlan } from "./useRemoteTeamPlan";
import { openWorkspacePlanCanvas } from "./workspacePlanEntry";

export function RemoteTeamPlanDrawer({ api, target, state, panelRef, onClose, onOpenCanvas }: {
  api: ApiConfig;
  target: PlanSidePanelTarget & { remoteTeam: RemoteTeamPlanTarget };
  state: PresenceState;
  panelRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  onOpenCanvas?: () => void;
}) {
  const { remoteTeam, taskId } = target;
  const remote = useRemoteTeamPlan(api, remoteTeam);
  const planId = remote.plan?.active_plan?.id ?? null;
  const selectedTask = taskId && target.planId === planId ? taskId : null;
  const list = usePlanListWindow(remote.peerApi, selectedTask ? null : planId, "all", "");
  const task = usePlanTaskDetail(remote.peerApi, selectedTask ? planId : null, selectedTask);
  const grant = remote.plan?.remote_plan_actions;
  const grantReason = remotePlanGrantReason(grant);
  const detailTarget = useMemo(() => ({ ...target, nestedFromPlan: true }), [target]);
  const viewCanvas = (openTask?: string) => {
    onClose();
    onOpenCanvas?.();
    openWorkspacePlanCanvas({
      remoteTeam,
      planId: planId ?? undefined,
      jump: openTask ? { view: "task", taskId: openTask } : { view: "board" },
    });
  };
  if (planId && selectedTask) return <DsRenderContext.Provider value={{ authority: remoteTeam.peerId, trusted: false, proseOnly: true }}>
    <PlanSidePanel
      target={detailTarget}
      api={remote.peerApi}
      plan={null}
      taskDetail={task.detail}
      taskLoading={task.loading}
      taskError={task.error}
      onRetryTask={task.retry}
      historyController={task}
      footer={task.detail && <RemotePlanTaskActions key={selectedTask} target={remoteTeam} detail={task.detail} grant={grant} workspaceId={remote.plan!.team.workspace_id} />}
      state={state}
      panelRef={panelRef}
      onClose={onClose}
      onOpenPlanTarget={next => next.presentation === "full" ? viewCanvas(next.taskId)
        : planSidePanelStore.open({ ...next, planId, remoteTeam })}
    />
  </DsRenderContext.Provider>;
  const title = remote.plan?.active_plan?.title ?? remoteTeam.name;
  return <SidePanelFrame state={state} panelRef={panelRef} onClose={onClose}
    label={`Task status for ${title}`}
    title={<h2 className="plan-side-panel__title">{title}</h2>}
    actions={<button type="button" className="plan-side-panel__action" aria-label="Open Plan canvas"
      title="Open Plan canvas" onClick={() => viewCanvas()}><Maximize2 size={14} aria-hidden /></button>}
  >
    {grantReason && remote.plan && <p role="status" className="px-4 py-2 text-xs">Read-only. {grantReason}</p>}
    <PlanFetchErrorState loading={remote.loading || list.loading} error={remote.error ?? list.error}
      itemCount={list.plan?.tasks.length} onRetry={remote.error ? remote.retry : list.retry} />
    {remote.plan && !planId && <p className="p-4">This Team has no active Plan.</p>}
    {list.plan && <ul className="divide-y divide-[var(--ct-border)] px-4">
      {list.plan.tasks.map(task => <li key={task.id}>
        <button type="button" className="flex w-full items-start justify-between gap-3 py-2 text-left"
          onClick={() => planSidePanelStore.open({ planId: list.plan!.id, taskId: task.id, remoteTeam })}>
          <span>{taskDisplayKey(task)} {task.name}</span>
          <span className="shrink-0 text-xs text-[var(--ct-muted-fg)]">{task.status_label || canonicalTaskStatusLabel(task)}</span>
        </button>
      </li>)}
    </ul>}
    {list.moreError && <p role="alert" className="px-4 py-2">{list.moreError}</p>}
    {list.plan?.task_window.next_cursor && <button type="button" className="m-4 rounded border px-3 py-1 text-xs"
      disabled={list.loadingMore} onClick={() => void list.loadMore()}>{list.loadingMore ? "Loading…" : "Load more tasks"}</button>}
  </SidePanelFrame>;
}
