import { useCallback, useEffect, useMemo, useState } from "react";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import { deleteTask, fetchSpecSections } from "../plansApi";
import { fixtureSpecSectionsByPlan } from "../perfFixture";
import type { Plan, Section, Task } from "../types";
import {
  boundDoneColumnCards,
  buildBoardPlanModel,
  DONE_COLUMN_COLLAPSED_LIMIT,
  filterPlansForBoard,
  orderPlansByActivity,
  selectDefaultPlanId,
  type BoardColumnKey,
  type BoardPlanModel,
} from "./boardModel";
import { BoardTaskCard, type BoardTaskActions } from "./BoardTaskCard";
import { CancelledTasksPanel } from "./CancelledTasksPanel";
import { listCancelledTasks } from "./cancelledTasks";
import { PlanSwitcher } from "./PlanSwitcher";
import { usePlanSwitcherActions } from "./usePlanSwitcherActions";
import { enrichSwitcherOptions, selectActiveWork } from "./activeWork";
import { PlanFetchErrorState, PlanFetchReconnectingBanner } from "./PlanFetchErrorState";
import { planFetchShowsData } from "./planViewFetchGate";

const SELECTED_PLAN_STORAGE_KEY = "codeterm.board.selectedPlanId";

function readStoredPlanId(): string | null {
  try {
    return window.sessionStorage.getItem(SELECTED_PLAN_STORAGE_KEY);
  } catch {
    return null;
  }
}

function BoardColumn({
  columnKey,
  label,
  cards,
  doneExpanded,
  onToggleDone,
  onOpenTask,
  planId,
  taskActions,
}: {
  columnKey: BoardColumnKey;
  label: string;
  cards: BoardPlanModel["columns"][number]["cards"];
  doneExpanded: boolean;
  onToggleDone: () => void;
  onOpenTask: (task: Task) => void;
  planId: string;
  taskActions: BoardTaskActions;
}) {
  const isDone = columnKey === "done";
  const bounds = isDone ? boundDoneColumnCards(cards, doneExpanded) : { visible: cards, total: cards.length, hidden: 0 };
  const count = isDone ? bounds.total : cards.length;

  return (
    <div
      data-testid="plan-board-column"
      data-column={columnKey}
      className="plan-board-column"
      role="listitem"
    >
      <header className="plan-board-column-head">
        <span data-testid="plan-board-column-label">{label}</span>
        <span data-testid="plan-board-column-count" className="plan-board-column-count">
          {count}
        </span>
      </header>
      <div className="plan-board-column-body">
        {bounds.visible.length === 0 && (
          <p data-testid="plan-board-column-empty" className="plan-board-column-empty" aria-hidden>
            —
          </p>
        )}
        {bounds.visible.map((card) => (
          <BoardTaskCard
            key={card.task.id}
            card={card}
            planId={planId}
            onOpen={() => onOpenTask(card.task)}
            {...taskActions}
          />
        ))}
        {isDone && bounds.hidden > 0 && (
          <button
            type="button"
            data-testid="plan-board-done-toggle"
            className="plan-board-done-toggle"
            onClick={onToggleDone}
          >
            Show all {bounds.total}
          </button>
        )}
        {isDone && doneExpanded && bounds.total > DONE_COLUMN_COLLAPSED_LIMIT && (
          <button
            type="button"
            data-testid="plan-board-done-toggle"
            className="plan-board-done-toggle"
            onClick={onToggleDone}
          >
            Show recent {DONE_COLUMN_COLLAPSED_LIMIT}
          </button>
        )}
      </div>
    </div>
  );
}

function BoardPlanBoard({
  model,
  plan,
  taskActions,
  onOpenTask,
}: {
  model: BoardPlanModel;
  plan: Plan;
  taskActions: BoardTaskActions;
  onOpenTask: (task: Task) => void;
}) {
  const [doneExpanded, setDoneExpanded] = useState(false);
  const [cancelledOpen, setCancelledOpen] = useState(false);
  const cancelledTasks = useMemo(() => listCancelledTasks(plan), [plan]);

  return (
    <section data-testid="plan-board-plan" data-plan-id={model.planId} className="plan-board-plan">
      <div data-testid="plan-board-columns" className="plan-board-columns" role="list">
        {model.columns.map((column) => (
          <BoardColumn
            key={column.key}
            columnKey={column.key}
            label={column.label}
            cards={column.cards}
            doneExpanded={doneExpanded}
            onToggleDone={() => setDoneExpanded((v) => !v)}
            onOpenTask={onOpenTask}
            planId={plan.id}
            taskActions={taskActions}
          />
        ))}
      </div>
      {model.cancelledCount > 0 && (
        <>
          <button
            type="button"
            data-testid="plan-board-cancelled-note"
            className="plan-board-cancelled-note plan-board-cancelled-trigger"
            onClick={() => setCancelledOpen((v) => !v)}
            aria-expanded={cancelledOpen}
          >
            {model.cancelledCount} cancelled task{model.cancelledCount === 1 ? "" : "s"} hidden from board
          </button>
          {cancelledOpen && <CancelledTasksPanel tasks={cancelledTasks} onOpenTask={onOpenTask} />}
        </>
      )}
    </section>
  );
}

export function PlanBoardView({
  mode,
  plans,
  loading = false,
  reconnecting = false,
  error = null,
  onRetry,
  planId,
  teamId,
  orchestratorTabId,
  tabs = {},
  workspaceId,
  workspaces,
  teams,
  onOpenPlanTarget,
  api,
}: PlanViewSurfaceProps) {
  const { effectivePlans, switcherOptions, handleAction } = usePlanSwitcherActions(plans, api);

  const taskActions = useMemo<BoardTaskActions>(() => (api
    ? {
        onDeleteTask: (selectedPlanId, taskId) => deleteTask(api, selectedPlanId, taskId),
      }
    : {}), [api]);

  const activeWork = useMemo(
    () => selectActiveWork(effectivePlans, tabs),
    [effectivePlans, tabs],
  );
  const switcherWithInflight = useMemo(
    () => enrichSwitcherOptions(switcherOptions, activeWork),
    [switcherOptions, activeWork],
  );
  const [manualPlanId, setManualPlanId] = useState<string | null>(readStoredPlanId);

  const filteredPlans = useMemo(
    () => {
      const active = filterPlansForBoard(effectivePlans, mode, { planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams });
      if (!manualPlanId || active.some((p) => p.id === manualPlanId)) return active;
      const archivedPlan = plans.find((p) => p.id === manualPlanId && p.status === "archived");
      if (!archivedPlan || (mode === "focus" && planId && planId !== archivedPlan.id)) return active;
      return [...active, archivedPlan];
    },
    [effectivePlans, manualPlanId, mode, planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams, plans],
  );

  const orderedPlans = useMemo(() => orderPlansByActivity(filteredPlans), [filteredPlans]);

  const defaultPlanId = useMemo(
    () => selectDefaultPlanId(orderedPlans, { planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams }),
    [orderedPlans, planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams],
  );

  const selectedPlanId = useMemo(() => {
    if (manualPlanId && orderedPlans.some((p) => p.id === manualPlanId)) return manualPlanId;
    return defaultPlanId;
  }, [manualPlanId, orderedPlans, defaultPlanId]);

  const selectedPlan = useMemo(
    () => orderedPlans.find((p) => p.id === selectedPlanId) ?? null,
    [orderedPlans, selectedPlanId],
  );

  const selectPlan = useCallback((id: string) => {
    setManualPlanId(id);
    try {
      window.sessionStorage.setItem(SELECTED_PLAN_STORAGE_KEY, id);
    } catch {
      // Session persistence is best-effort; private-mode/quota failures must not block switching.
    }
  }, []);

  const fixtureSpecSections = useMemo(
    () => (selectedPlan ? fixtureSpecSectionsByPlan([selectedPlan]) : new Map<string, Section[]>()),
    [selectedPlan],
  );
  const [fetchedSpecSections, setFetchedSpecSections] = useState<Map<string, Section[]>>(new Map());

  // Primitive keys only: mux rebuilds the plan object on every broadcast, and
  // this effect reaches the API.
  const specPlanId = selectedPlan?.id ?? "";
  const specSourceId = selectedPlan?.primary_spec_id ?? "";
  const specIsFixture = specPlanId ? fixtureSpecSections.has(specPlanId) : false;
  useEffect(() => {
    if (!api || !specSourceId || specIsFixture) return;
    let active = true;
    void fetchSpecSections(api, specSourceId)
      .then((sections) => (active ? [specPlanId, sections] as const : null))
      .catch(() => (active ? [specPlanId, [] as Section[]] as const : null))
      .then((pair) => {
        if (!active || !pair) return;
        setFetchedSpecSections((prev) => new Map(prev).set(pair[0], pair[1]));
      });
    return () => {
      active = false;
    };
  }, [api?.host, api?.port, api?.token, specPlanId, specSourceId, specIsFixture]);

  const selectedSpecSections = useMemo(() => {
    if (!selectedPlan) return [] as Section[];
    return fixtureSpecSections.get(selectedPlan.id) ?? fetchedSpecSections.get(selectedPlan.id) ?? [];
  }, [selectedPlan, fixtureSpecSections, fetchedSpecSections]);

  const boardModel = useMemo(
    () => (selectedPlan ? buildBoardPlanModel(selectedPlan, tabs, selectedSpecSections) : null),
    [selectedPlan, tabs, selectedSpecSections],
  );

  const openTask = useCallback((task: Task) => {
    if (!selectedPlan) return;
    onOpenPlanTarget?.({ planId: selectedPlan.id, taskId: task.id, teamId: selectedPlan.team_id });
  }, [selectedPlan, onOpenPlanTarget]);

  const showsData = planFetchShowsData(loading, error ?? null, plans.length);

  return (
    <div data-testid="plan-board-view" className="plan-board-view">
      <PlanFetchErrorState
        loading={loading}
        error={error}
        itemCount={plans.length}
        onRetry={onRetry}
        testId="plan-board"
        className="plan-board-state"
      />
      <PlanFetchReconnectingBanner
        reconnecting={reconnecting}
        itemCount={plans.length}
        testId="plan-board-reconnecting"
        className="plan-board-state px-3 py-1"
      />
      {showsData && !selectedPlan && (
        <p data-testid="plan-board-empty" className="plan-board-state">
          No active plans in this workspace.
        </p>
      )}
      {showsData && selectedPlan && boardModel && (
        <div data-testid="plan-board-single" className="plan-board-single">
          {!teamId && (
            <PlanSwitcher
              plans={switcherWithInflight}
              selectedId={selectedPlan.id}
              onSelect={selectPlan}
              onAction={handleAction}
            />
          )}
          <BoardPlanBoard
            model={boardModel}
            plan={selectedPlan}
            taskActions={taskActions}
            onOpenTask={openTask}
          />
        </div>
      )}
    </div>
  );
}
