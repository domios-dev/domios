import { memo, useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { X } from "lucide-react";
import { PlanViewModeSwitch, type TemporaryModeTab } from "./PlanViewModeSwitch";
import type { PlanViewSurfaceProps } from "./planViewSurfaceProps";
import type { CanvasWorkspaceTarget } from "./canvasFocusStore";
import { MissionControlPlanSurface } from "./missionControl/MissionControlPlanSurface";
import {
  DEFAULT_PLAN_VIEW_MODE,
  resolvePlanViewMode,
  type PlanViewMode,
} from "./planViewMode";
import { PlanBoardView } from "./planView/PlanBoardView";
import { PlanListView } from "./planView/PlanListView";
import { PlanPlanView, PlanSpecView } from "./planView/PlanDocumentView";
import { TaskSegmentPanel } from "./planView/TaskSegmentPanel";
import { findTaskByReference, taskSegmentLabel } from "./taskDisplayKey";
import { isTeamActivePlan } from "./planTabMatch";
import type { Plan } from "./types";

const BoardSurface = memo(MissionControlPlanSurface);
const ListSurface = memo(PlanListView);
const SpecSurface = memo(PlanSpecView);
const PlanSurface = memo(PlanPlanView);
const TasksSurface = memo(PlanBoardView);

const PLAN_VIEW_MODE_STORAGE_KEY = "codeterm.planViewMode";
const TASK_SEGMENT_VALUE = "task";

interface TaskSegment {
  planId: string;
  taskRef: string;
}

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

/** Each navigation target is a new object, so re-opening the same task after closing it lands again. */
function useTaskSegment(
  target: CanvasWorkspaceTarget | null | undefined,
  setViewMode: (mode: PlanViewMode) => void,
): [TaskSegment | null, (segment: TaskSegment | null) => void] {
  const [segment, setSegment] = useState<TaskSegment | null>(null);
  useLayoutEffect(() => {
    const planId = target?.planId;
    const jump = target?.jump;
    if (!planId || !jump) return;
    if (jump.view === "task") {
      setSegment({ planId, taskRef: jump.taskId });
      return;
    }
    setSegment(null);
    setViewMode("plan");
  }, [target, setViewMode]);
  return [segment, setSegment];
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
  const [segment, setSegment] = useTaskSegment(props.workspaceTarget, setViewMode);
  const hasActivePlan = useMemo(
    () => props.plans.some((plan) => isTeamActivePlan(plan, props.teamId)),
    [props.plans, props.teamId],
  );
  // The plan views need a plan; the stored choice returns once a plan is active again.
  const viewMode: PlanViewMode = hasActivePlan ? storedMode : "board";
  const selected = segment ? TASK_SEGMENT_VALUE : viewMode;
  const listProps = useFrozenWhenHidden(props, selected === "list");
  const specProps = useFrozenWhenHidden(props, selected === "spec");
  const planProps = useFrozenWhenHidden(props, selected === "plan");
  const boardProps = useFrozenWhenHidden(props, selected === "board");
  const tasksProps = useFrozenWhenHidden(props, selected === "tasks");
  const [actionsSlot, setActionsSlot] = useState<HTMLDivElement | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const panelRef = useRef<HTMLElement | null>(null);
  const focusSelectedModeRef = useRef(false);
  const segmentTabId = useId();
  const segmentPanelId = useId();

  const selectMode = useCallback((mode: PlanViewMode) => {
    setSegment(null);
    setViewMode(mode);
  }, [setSegment, setViewMode]);
  const openSpec = useCallback(() => selectMode("spec"), [selectMode]);
  const closeSegment = useCallback(() => {
    focusSelectedModeRef.current = true;
    selectMode("tasks");
  }, [selectMode]);

  useEffect(() => {
    if (segment) {
      panelRef.current?.focus();
      return;
    }
    if (!focusSelectedModeRef.current) return;
    focusSelectedModeRef.current = false;
    rootRef.current?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
  }, [segment]);

  const found = useMemo(() => {
    if (!segment) return null;
    const plan: Plan | undefined = props.plans.find((candidate) => candidate.id === segment.planId);
    const task = plan ? findTaskByReference(plan.tasks, segment.taskRef) : undefined;
    return plan && task ? { plan, task } : null;
  }, [props.plans, segment]);

  const temporaryTab = useMemo<TemporaryModeTab | null>(() => {
    if (!segment) return null;
    const label = taskSegmentLabel({ id: segment.taskRef, key: found?.task.key });
    return {
      value: TASK_SEGMENT_VALUE,
      label,
      closeLabel: `Close ${label}`,
      tabId: segmentTabId,
      panelId: segmentPanelId,
      onClose: closeSegment,
    };
  }, [segment, found, segmentTabId, segmentPanelId, closeSegment]);

  const { onOpenPlanTarget } = props;
  const openRelatedTask = useCallback((taskId: string) => {
    if (!found) return;
    onOpenPlanTarget?.({ planId: found.plan.id, taskId, teamId: found.plan.team_id });
  }, [found, onOpenPlanTarget]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!segment || event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    closeSegment();
  };

  return (
    <div
      ref={rootRef}
      data-testid="plan-view-shell"
      data-plan-view-mode={selected}
      className="flex h-full w-full flex-col overflow-hidden"
      onKeyDown={onKeyDown}
    >
      <header
        className="flex shrink-0 items-center justify-between gap-3 border-b px-3 py-2"
        style={{
          borderColor: "var(--ct-border)",
          background: "var(--ct-titlebar-bg)",
        }}
      >
        <PlanViewModeSwitch
          value={selected}
          onChange={selectMode}
          planModesEnabled={hasActivePlan}
          temporaryTab={temporaryTab}
        />
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
          hidden={selected !== "spec"}
          aria-hidden={selected !== "spec"}
        >
          <SpecSurface {...specProps} />
        </div>
        <div
          className="absolute inset-0"
          hidden={selected !== "plan"}
          aria-hidden={selected !== "plan"}
        >
          <PlanSurface {...planProps} onOpenSpec={openSpec} />
        </div>
        <div
          className="absolute inset-0"
          hidden={selected !== "board"}
          aria-hidden={selected !== "board"}
        >
          <BoardSurface
            {...boardProps}
            headerActionsSlot={selected === "board" ? actionsSlot : null}
            onOpenView={selectMode}
          />
        </div>
        <div
          className="absolute inset-0"
          hidden={selected !== "list"}
          aria-hidden={selected !== "list"}
        >
          <ListSurface {...listProps} onClose={undefined} />
        </div>
        <div
          className="absolute inset-0"
          hidden={selected !== "tasks"}
          aria-hidden={selected !== "tasks"}
        >
          <TasksSurface {...tasksProps} />
        </div>
        {segment && (
          <div className="absolute inset-0">
            <TaskSegmentPanel
              panelId={segmentPanelId}
              tabId={segmentTabId}
              panelRef={panelRef}
              plan={found?.plan ?? null}
              task={found?.task ?? null}
              taskRef={segment.taskRef}
              loading={props.loading}
              api={props.api}
              tabs={props.tabs}
              onOpenTab={props.onOpenTab}
              onOpenTask={openRelatedTask}
              onOpenSection={selectMode}
            />
          </div>
        )}
      </div>
    </div>
  );
}
