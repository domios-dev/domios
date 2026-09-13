import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import { deleteTask, fetchSpecSections, pauseTask, resumeTask } from "../plansApi";
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
import { PlanDetailOverlay, type PlanDetailKind } from "./PlanDetailOverlay";
import { PlanFetchErrorState, PlanFetchReconnectingBanner } from "./PlanFetchErrorState";
import { planFetchShowsData } from "./planViewFetchGate";
import { EMPTY_NAV_STACK, navCanGoBack, navPop, navPush, type NavEntry, type NavStack } from "./navStack";

const SELECTED_PLAN_STORAGE_KEY = "codeterm.board.selectedPlanId";

function readStoredPlanId(): string | null {
  try {
    return window.sessionStorage.getItem(SELECTED_PLAN_STORAGE_KEY);
  } catch {
    return null;
  }
}

type BoardDetail =
  | { kind: "task"; task: Task; plan: Plan }
  | { kind: "plan"; plan: Plan; specSections: Section[] }
  | { kind: "spec"; plan: Plan; specSections: Section[] };

function boardDetailToEntry(detail: BoardDetail, scroll?: number): NavEntry {
  if (detail.kind === "task") {
    return { surface: "task", id: detail.task.id, planId: detail.plan.id, scroll };
  }
  return { surface: detail.kind, id: detail.plan.id, planId: detail.plan.id, scroll };
}

function entryToBoardDetail(
  entry: NavEntry,
  plans: Plan[],
  specSections: Section[],
): BoardDetail | null {
  const plan = plans.find((p) => p.id === entry.planId);
  if (!plan) return null;
  if (entry.surface === "task") {
    const task = plan.tasks.find((t) => t.id === entry.id);
    if (!task) return null;
    return { kind: "task", task, plan };
  }
  return { kind: entry.surface, plan, specSections };
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
  onOpenDetail,
}: {
  model: BoardPlanModel;
  plan: Plan;
  taskActions: BoardTaskActions;
  onOpenDetail: (detail: BoardDetail) => void;
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
            onOpenTask={(task) => onOpenDetail({ kind: "task", task, plan })}
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
          {cancelledOpen && (
            <CancelledTasksPanel
              tasks={cancelledTasks}
              onOpenTask={(task) => onOpenDetail({ kind: "task", task, plan })}
            />
          )}
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
  onOpenTab,
  api,
}: PlanViewSurfaceProps) {
  const { effectivePlans, switcherOptions, handleAction } = usePlanSwitcherActions(plans, api);

  const taskActions = useMemo<BoardTaskActions>(() => (api
    ? {
        onPauseTask: (selectedPlanId, taskId, reason) => pauseTask(api, selectedPlanId, taskId, reason),
        onResumeTask: (selectedPlanId, taskId) => resumeTask(api, selectedPlanId, taskId),
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
  const [detail, setDetail] = useState<BoardDetail | null>(null);
  const [navStack, setNavStack] = useState<NavStack>(EMPTY_NAV_STACK);
  const bodyScrollRef = useRef<HTMLDivElement | null>(null);
  const [restoreScroll, setRestoreScroll] = useState<number | undefined>(undefined);

  useEffect(() => {
    setNavStack(EMPTY_NAV_STACK);
    setDetail(null);
  }, [selectedPlanId]);

  useEffect(() => {
    if (!api || !selectedPlan?.primary_spec_id) return;
    if (fixtureSpecSections.has(selectedPlan.id)) return;
    let active = true;
    const planId = selectedPlan.id;
    const specId = selectedPlan.primary_spec_id;
    void fetchSpecSections(api, specId)
      .then((sections) => (active ? [planId, sections] as const : null))
      .catch(() => (active ? [planId, [] as Section[]] as const : null))
      .then((pair) => {
        if (!active || !pair) return;
        setFetchedSpecSections((prev) => new Map(prev).set(pair[0], pair[1]));
      });
    return () => {
      active = false;
    };
  }, [api, selectedPlan, fixtureSpecSections]);

  const selectedSpecSections = useMemo(() => {
    if (!selectedPlan) return [] as Section[];
    return fixtureSpecSections.get(selectedPlan.id) ?? fetchedSpecSections.get(selectedPlan.id) ?? [];
  }, [selectedPlan, fixtureSpecSections, fetchedSpecSections]);

  const boardModel = useMemo(
    () => (selectedPlan ? buildBoardPlanModel(selectedPlan, tabs, selectedSpecSections) : null),
    [selectedPlan, tabs, selectedSpecSections],
  );

  const closeDetail = useCallback(() => {
    setDetail(null);
    setNavStack(EMPTY_NAV_STACK);
  }, []);

  const pushCurrentDetail = useCallback((next: BoardDetail) => {
    if (detail) {
      const scroll = bodyScrollRef.current?.scrollTop ?? 0;
      setNavStack((prev) => navPush(prev, boardDetailToEntry(detail, scroll)));
    }
    setRestoreScroll(undefined);
    setDetail(next);
  }, [detail]);

  const openDetailTask = useCallback((taskId: string) => {
    if (!selectedPlan) return;
    const task = selectedPlan.tasks.find((candidate) => candidate.id === taskId);
    if (task) pushCurrentDetail({ kind: "task", task, plan: selectedPlan });
  }, [selectedPlan, pushCurrentDetail]);

  const openDetailSection = useCallback((args: { variant: "plan" | "spec" }) => {
    if (!selectedPlan) return;
    const next: BoardDetail = args.variant === "spec"
      ? { kind: "spec", plan: selectedPlan, specSections: selectedSpecSections }
      : { kind: "plan", plan: selectedPlan, specSections: selectedSpecSections };
    pushCurrentDetail(next);
  }, [selectedPlan, selectedSpecSections, pushCurrentDetail]);

  const handleBack = useCallback(() => {
    const { stack, entry } = navPop(navStack);
    if (!entry) return;
    const restored = entryToBoardDetail(entry, orderedPlans, selectedSpecSections);
    if (!restored) return;
    setNavStack(stack);
    setRestoreScroll(entry.scroll);
    setDetail(restored);
  }, [navStack, orderedPlans, selectedSpecSections]);

  const overlayKind: PlanDetailKind | null = detail?.kind ?? null;
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
            onOpenDetail={setDetail}
          />
        </div>
      )}
      {detail && overlayKind && (
        <PlanDetailOverlay
          kind={overlayKind}
          task={detail.kind === "task" ? detail.task : undefined}
          plan={detail.kind === "plan" ? detail.plan : detail.kind === "task" ? detail.plan : undefined}
          spec={detail.kind === "spec" ? detail.plan : undefined}
          specSections={detail.kind === "spec" || detail.kind === "plan" ? detail.specSections : undefined}
          planId={detail.plan.id}
          api={api}
          tabs={tabs}
          onOpenTab={onOpenTab}
          onOpenTask={openDetailTask}
          onClose={closeDetail}
          onSectionClick={openDetailSection}
          canGoBack={navCanGoBack(navStack)}
          onBack={handleBack}
          restoreScroll={restoreScroll}
          scrollCaptureRef={bodyScrollRef}
        />
      )}
    </div>
  );
}
