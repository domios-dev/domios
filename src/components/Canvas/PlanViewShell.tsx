import { memo, useCallback, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { PlanViewModeSwitch } from "./PlanViewModeSwitch";
import type { PlanViewSurfaceProps } from "./planViewSurfaceProps";
import { MissionControlPlanSurface } from "./missionControl/MissionControlPlanSurface";
import {
  DEFAULT_PLAN_VIEW_MODE,
  resolvePlanViewMode,
  type PlanViewMode,
} from "./planViewMode";
import { PlanBoardView } from "./planView/PlanBoardView";
import { PlanListView } from "./planView/PlanListView";
import { PlanPlanView, PlanSpecView } from "./planView/PlanDocumentView";

const BoardSurface = memo(MissionControlPlanSurface);
const ListSurface = memo(PlanListView);
const SpecSurface = memo(PlanSpecView);
const PlanSurface = memo(PlanPlanView);
const TasksSurface = memo(PlanBoardView);

const PLAN_VIEW_MODE_STORAGE_KEY = "codeterm.planViewMode";

function readPersistedPlanViewMode(): PlanViewMode {
  try {
    return resolvePlanViewMode(window.localStorage.getItem(PLAN_VIEW_MODE_STORAGE_KEY));
  } catch {
    return DEFAULT_PLAN_VIEW_MODE;
  }
}

function usePersistedPlanViewMode(): [PlanViewMode, (mode: PlanViewMode) => void] {
  const [viewMode, setViewModeState] = useState<PlanViewMode>(readPersistedPlanViewMode);

  const setViewMode = useCallback((mode: PlanViewMode) => {
    setViewModeState(mode);
    try {
      window.localStorage.setItem(PLAN_VIEW_MODE_STORAGE_KEY, mode);
    } catch {
      // Persistence is best-effort; quota/private-mode failures must not block switching.
    }
  }, []);

  return [viewMode, setViewMode];
}

/** Hidden surfaces keep the props they last rendered with: identical references let memo skip them entirely, so only the visible view pays for live plan ticks. */
export function useFrozenWhenHidden<T>(props: T, visible: boolean): T {
  const lastVisible = useRef(props);
  if (visible) {
    lastVisible.current = props;
  }
  return visible ? props : lastVisible.current;
}

export function PlanViewShell(props: PlanViewSurfaceProps) {
  const [storedMode, setViewMode] = usePersistedPlanViewMode();
  const hasActivePlan = useMemo(
    () => props.plans.some((plan) => plan.status === "active" && (!props.teamId || plan.team_id === props.teamId)),
    [props.plans, props.teamId],
  );
  // The plan views need a plan; the stored choice returns once a plan is active again.
  const viewMode: PlanViewMode = hasActivePlan ? storedMode : "board";
  const listProps = useFrozenWhenHidden(props, viewMode === "list");
  const specProps = useFrozenWhenHidden(props, viewMode === "spec");
  const planProps = useFrozenWhenHidden(props, viewMode === "plan");
  const boardProps = useFrozenWhenHidden(props, viewMode === "board");
  const tasksProps = useFrozenWhenHidden(props, viewMode === "tasks");
  const openSpec = useCallback(() => setViewMode("spec"), [setViewMode]);
  const [actionsSlot, setActionsSlot] = useState<HTMLDivElement | null>(null);

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
        <PlanViewModeSwitch value={viewMode} onChange={setViewMode} planModesEnabled={hasActivePlan} />
        <div className="flex shrink-0 items-center gap-2">
          <div
            ref={setActionsSlot}
            data-testid="plan-view-header-actions"
            className="plan-view-header-actions flex items-center gap-2"
          />
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
        </div>
      </header>
      <div className="relative min-h-0 flex-1">
        <div
          className="absolute inset-0"
          hidden={viewMode !== "spec"}
          aria-hidden={viewMode !== "spec"}
        >
          <SpecSurface {...specProps} />
        </div>
        <div
          className="absolute inset-0"
          hidden={viewMode !== "plan"}
          aria-hidden={viewMode !== "plan"}
        >
          <PlanSurface {...planProps} onOpenSpec={openSpec} />
        </div>
        <div
          className="absolute inset-0"
          hidden={viewMode !== "board"}
          aria-hidden={viewMode !== "board"}
        >
          <BoardSurface
            {...boardProps}
            headerActionsSlot={viewMode === "board" ? actionsSlot : null}
            onOpenView={setViewMode}
          />
        </div>
        <div
          className="absolute inset-0"
          hidden={viewMode !== "list"}
          aria-hidden={viewMode !== "list"}
        >
          <ListSurface {...listProps} onClose={undefined} />
        </div>
        <div
          className="absolute inset-0"
          hidden={viewMode !== "tasks"}
          aria-hidden={viewMode !== "tasks"}
        >
          <TasksSurface {...tasksProps} />
        </div>
      </div>
    </div>
  );
}
