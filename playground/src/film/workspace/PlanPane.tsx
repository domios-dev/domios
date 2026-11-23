import { useMemo } from "react";
import { PlanViewShell } from "../../../../src/components/Canvas/PlanViewShell";
import type { CanvasWorkspaceTarget } from "../../../../src/components/Canvas/canvasFocusStore";
import type { Plan } from "../../../../src/components/Canvas/types";
import type { PlanViewMode } from "../../../../src/components/Canvas/planViewMode";
import type { Tab, Team, Workspace } from "../../../../src/lib/types";
import { API, ProductProviders } from "../../gallery/fixtures";

const MODE_KEY = "codeterm.planViewMode";

export interface PlanPaneProps {
  plan: Plan;
  mode: Extract<PlanViewMode, "tasks" | "board">;
  /** A fresh object per open, as the app's own navigation targets are, so re-opening a task lands again. */
  target: { taskId: string; serial: number } | null;
  tabs: Tab[];
  teams: Team[];
  workspaces: Workspace[];
  workspaceId: string;
  onOpenTask: (taskId: string) => void;
  onOpenTab: (tabId: string) => void;
}

/** CanvasView's plan surface: the shipped PlanViewShell with its Board, Spec, Plan and Tasks views over the team's plan. */
export function PlanPane({ plan, mode, target, tabs, teams, workspaces, workspaceId, onOpenTask, onOpenTab }: PlanPaneProps) {
  // The shell restores its last view from storage when it mounts; the pane remounts it per mode with that view stored.
  localStorage.setItem(MODE_KEY, mode);
  const tabMap = useMemo(() => Object.fromEntries(tabs.map((tab) => [tab.id, tab])), [tabs]);
  const plans = useMemo(() => [plan], [plan]);
  const workspaceTarget = useMemo<CanvasWorkspaceTarget | null>(
    () => (target ? { planId: plan.id, teamId: plan.team_id ?? undefined, jump: { view: "task", taskId: target.taskId } } : null),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a new target serial is a new navigation; plan ticks are not
    [target?.serial, plan.id],
  );
  return <ProductProviders tabs={tabs} workspaceId={workspaceId}>
    <div className="h-full w-full" style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
      <PlanViewShell
        key={mode}
        mode="workspace"
        plans={plans}
        planId={plan.id}
        teamId={plan.team_id ?? undefined}
        orchestratorTabId={plan.owner?.tab_id ?? undefined}
        tabs={tabMap}
        workspaces={workspaces}
        workspaceId={workspaceId}
        teams={teams}
        projectPath={plan.project_path ?? null}
        workspaceTarget={workspaceTarget}
        onOpenTab={onOpenTab}
        onOpenPlanTarget={({ taskId }) => taskId && onOpenTask(taskId)}
        api={API}
      />
    </div>
  </ProductProviders>;
}
