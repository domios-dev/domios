import { useMemo, useState } from "react";
import { X } from "lucide-react";
import { CanvasViewInner, type CanvasViewInnerProps } from "./CanvasView";
import { PlanViewModeSwitch } from "./PlanViewModeSwitch";
import { DEFAULT_PLAN_VIEW_MODE, type PlanViewMode } from "./planViewMode";
import type { PlanViewSurfaceProps } from "./planViewSurfaceProps";
import { PlanBoardView } from "./planView/PlanBoardView";
import { PlanListView } from "./planView/PlanListView";

function toSurfaceProps(props: CanvasViewInnerProps): PlanViewSurfaceProps {
  return {
    mode: props.mode,
    plans: props.plans,
    loading: props.loading,
    error: props.error,
    planId: props.planId,
    orchestratorPaneId: props.orchestratorPaneId,
    focusPaneId: props.focusPaneId,
    panes: props.panes,
    workspaces: props.workspaces,
    focusedPaneId: props.focusedPaneId,
    projectPath: props.projectPath,
    workspaceTarget: props.workspaceTarget,
    onClose: props.onClose,
    onOpenPane: props.onOpenPane,
    api: props.api,
  };
}

export function PlanViewShell(props: CanvasViewInnerProps) {
  const [viewMode, setViewMode] = useState<PlanViewMode>(DEFAULT_PLAN_VIEW_MODE);
  const surfaceProps = useMemo(() => toSurfaceProps(props), [props]);

  return (
    <div
      data-testid="plan-view-shell"
      data-plan-view-mode={viewMode}
      className="flex h-full w-full flex-col overflow-hidden"
    >
      <header
        className="flex shrink-0 items-center justify-between gap-3 border-b px-3 py-2"
        style={{
          borderColor: "var(--ct-border)",
          background: "var(--ct-titlebar-bg)",
        }}
      >
        <PlanViewModeSwitch value={viewMode} onChange={setViewMode} />
        {props.onClose && (
          <button
            type="button"
            data-testid="plan-view-close"
            onClick={props.onClose}
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded border cursor-pointer"
            style={{ background: "var(--ct-bg)", borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
            title="Close plan"
          >
            <X size={14} />
          </button>
        )}
      </header>
      <div className="relative min-h-0 flex-1">
        <div
          className="absolute inset-0"
          hidden={viewMode !== "canvas"}
          aria-hidden={viewMode !== "canvas"}
        >
          <CanvasViewInner {...props} onClose={undefined} />
        </div>
        <div
          className="absolute inset-0"
          hidden={viewMode !== "list"}
          aria-hidden={viewMode !== "list"}
        >
          <PlanListView {...surfaceProps} onClose={undefined} />
        </div>
        <div
          className="absolute inset-0"
          hidden={viewMode !== "board"}
          aria-hidden={viewMode !== "board"}
        >
          <PlanBoardView {...surfaceProps} />
        </div>
      </div>
    </div>
  );
}
