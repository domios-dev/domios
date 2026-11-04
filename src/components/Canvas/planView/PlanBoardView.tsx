import type { PlanTaskView as Task } from "../planProjection";
import type { Section } from "../types";
import type { PlanView as Plan } from "../planProjection";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { Tab } from "../../../lib/types";
import { Loader } from "../../shared/Loader";
import { InlineErrorNotice } from "../../shared/ErrorNotice";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import { archivePlan, deleteTask, fetchSpecSections, renamePlan, unarchivePlan } from "../plansApi";
import type { PlanIndexEntry } from "../plansApi";
import { fixtureSpecSectionsByPlan } from "../perfFixture";
import { type PlanStatus, type PlanTaskBoardColumnKey, type PlanTaskBoardSummary } from "../types";
import { usePlanTaskBoard } from "../usePlans";
import { collectTabChain } from "../planTabMatch";
import {
  BOARD_COLUMN_DEFS,
  boundDoneColumnCards,
  buildSummaryBoardTaskCard,
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
import { usePlanSwitcherActions } from "./usePlanSwitcherActions";
import { enrichSwitcherOptions, selectActiveWork } from "./activeWork";
import { PlanFetchErrorState, PlanFetchReconnectingBanner } from "./PlanFetchErrorState";
import { planFetchShowsData } from "./planViewFetchGate";
import { PlanSwitcher, type PlanKebabAction, type PlanSwitcherOption } from "./PlanSwitcher";

const SELECTED_PLAN_STORAGE_KEY = "codeterm.board.selectedPlanId";
type TaskSelection = Pick<Task, "id">;

function readStoredPlanId(): string | null {
  try {
    return window.sessionStorage.getItem(SELECTED_PLAN_STORAGE_KEY);
  } catch {
    return null;
  }
}

function comparableProjectPath(path?: string | null): string {
  const normalized = (path ?? "").replace(/\\/g, "/").replace(/^\/\/\?\//, "").replace(/\/+$/, "");
  return /^[a-z]:\//i.test(normalized) ? normalized.toLowerCase() : normalized;
}

function indexEntryInWorkspace(
  entry: PlanIndexEntry,
  workspaceId: string,
  workspaces: PlanViewSurfaceProps["workspaces"],
  teams: PlanViewSurfaceProps["teams"],
): boolean {
  if (entry.team_id) {
    const teamWorkspaceId = teams?.find((team) => team.id === entry.team_id)?.workspace_id;
    if (teamWorkspaceId === workspaceId) return true;
  }
  const ownerTabId = entry.owner.tab_id;
  return Boolean(ownerTabId && workspaces?.some((workspace) =>
    workspace.id === workspaceId && workspace.tabs.includes(ownerTabId),
  ));
}

function filterPlanIndex(
  entries: readonly PlanIndexEntry[],
  props: PlanViewSurfaceProps,
): PlanIndexEntry[] {
  let filtered = [...entries];
  if (props.mode === "workspace") {
    const project = comparableProjectPath(props.projectPath);
    if (project) {
      filtered = filtered.filter((entry) =>
        comparableProjectPath(entry.canonical_project_path) === project ||
        Boolean(props.workspaceId && indexEntryInWorkspace(entry, props.workspaceId, props.workspaces, props.teams)),
      );
    }
    if (props.teamId) return filtered.filter((entry) => entry.team_id === props.teamId);
    if (!props.workspaceId) return [];
    filtered = filtered.filter((entry) => indexEntryInWorkspace(entry, props.workspaceId!, props.workspaces, props.teams));
    return filtered;
  }
  if (props.planId) return filtered.filter((entry) => entry.plan_id === props.planId);
  if (props.orchestratorTabId && props.tabs) {
    const chain = collectTabChain(props.orchestratorTabId, props.tabs);
    const tabIds = new Set(chain.map((tab) => tab.id));
    const sessionIds = new Set(chain.map((tab) => tab.agent_session_id).filter((id): id is string => Boolean(id)));
    filtered = filtered.filter((entry) =>
      Boolean(entry.owner.tab_id && tabIds.has(entry.owner.tab_id)) ||
      Boolean(entry.owner.session_id && sessionIds.has(entry.owner.session_id)),
    );
  }
  return filtered;
}

function orderPlanIndex(entries: readonly PlanIndexEntry[]): PlanIndexEntry[] {
  return [...entries].sort((left, right) =>
    right.updated_at - left.updated_at ||
    left.title.localeCompare(right.title) ||
    left.plan_id.localeCompare(right.plan_id),
  );
}

function selectIndexPlanId(entries: readonly PlanIndexEntry[], props: PlanViewSurfaceProps): string | null {
  if (!entries.length) return null;
  if (props.planId && entries.some((entry) => entry.plan_id === props.planId)) return props.planId;
  if (props.orchestratorTabId && props.tabs) {
    const chain = collectTabChain(props.orchestratorTabId, props.tabs);
    const tabIds = new Set(chain.map((tab) => tab.id));
    const sessionIds = new Set(chain.map((tab) => tab.agent_session_id).filter((id): id is string => Boolean(id)));
    const owned = entries.find((entry) =>
      Boolean(entry.owner.tab_id && tabIds.has(entry.owner.tab_id)) ||
      Boolean(entry.owner.session_id && sessionIds.has(entry.owner.session_id)),
    );
    if (owned) return owned.plan_id;
  }
  return (entries.find((entry) => (entry.open_task_count ?? 0) > 0) ??
    entries.find((entry) => (entry.task_count ?? 0) > 0) ?? entries[0]).plan_id;
}

function SummaryTaskCard({
  task,
  planId,
  tabs,
  onOpen,
  onDeleteTask,
}: {
  task: PlanTaskBoardSummary;
  planId: string;
  tabs: Record<string, Tab>;
  onOpen: () => void;
  onDeleteTask?: (planId: string, taskId: string) => void | Promise<void>;
}) {
  return (
    <BoardTaskCard
      card={buildSummaryBoardTaskCard(task, tabs)}
      planId={planId}
      onOpen={onOpen}
      onDeleteTask={onDeleteTask}
    />
  );
}

function SummaryBoardColumn({
  columnKey,
  label,
  count,
  items,
  expanded,
  loading,
  error,
  canLoadMore,
  onToggleExpanded,
  onLoadMore,
  onOpenTask,
  planId,
  tabs,
  onDeleteTask,
}: {
  columnKey: PlanTaskBoardColumnKey;
  label: string;
  count: number;
  items: PlanTaskBoardSummary[];
  expanded: boolean;
  loading: boolean;
  error: string | null;
  canLoadMore: boolean;
  onToggleExpanded: () => void;
  onLoadMore: () => void;
  onOpenTask: (task: PlanTaskBoardSummary) => void;
  planId: string;
  tabs: Record<string, Tab>;
  onDeleteTask?: (planId: string, taskId: string) => void | Promise<void>;
}) {
  const visible = columnKey === "done" && !expanded ? items.slice(0, DONE_COLUMN_COLLAPSED_LIMIT) : items;
  const hiddenLoaded = items.length - visible.length;
  return (
    <div data-testid="plan-board-column" data-column={columnKey} className="plan-board-column" role="listitem">
      <header className="plan-board-column-head">
        <span data-testid="plan-board-column-label">{label}</span>
        <span data-testid="plan-board-column-count" className="plan-board-column-count">{count}</span>
      </header>
      <div className="plan-board-column-body">
        {count === 0 && <p data-testid="plan-board-column-empty" className="plan-board-column-empty" aria-hidden>—</p>}
        {visible.map((task) => <SummaryTaskCard key={task.id} task={task} planId={planId} tabs={tabs} onOpen={() => onOpenTask(task)} onDeleteTask={onDeleteTask} />)}
        {columnKey === "done" && count > DONE_COLUMN_COLLAPSED_LIMIT && !expanded && (
          <button type="button" data-testid="plan-board-done-toggle" className="plan-board-done-toggle" onClick={onToggleExpanded}>
            Show loaded tasks ({items.length} of {count})
          </button>
        )}
        {columnKey === "done" && expanded && count > DONE_COLUMN_COLLAPSED_LIMIT && (
          <button type="button" data-testid="plan-board-done-toggle" className="plan-board-done-toggle" onClick={onToggleExpanded}>
            Show recent {DONE_COLUMN_COLLAPSED_LIMIT}
          </button>
        )}
        {hiddenLoaded > 0 && <span className="sr-only">{hiddenLoaded} loaded tasks hidden</span>}
        {canLoadMore && (
          <button type="button" data-testid={`plan-board-load-more-${columnKey}`} disabled={loading} onClick={onLoadMore} className="plan-board-done-toggle">
            {loading ? "Loading tasks…" : `Load more (${items.length} of ${count})`}
          </button>
        )}
        {error && <div className="px-2 py-1"><InlineErrorNotice error={error} operation="loading more tasks" /><button type="button" onClick={onLoadMore}>Retry</button></div>}
      </div>
    </div>
  );
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
  onOpenTask: (task: TaskSelection) => void;
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
  onOpenTask: (task: TaskSelection) => void;
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
  planIndex,
  loading = false,
  reconnecting = false,
  error = null,
  onRetry,
  planId,
  teamId,
  orchestratorTabId,
  tabs = {},
  workspaceId,
  projectPath,
  workspaces,
  teams,
  onOpenPlanTarget,
  api,
}: PlanViewSurfaceProps) {
  const pagedMode = planIndex !== undefined;
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
  const [indexStatusOverrides, setIndexStatusOverrides] = useState<ReadonlyMap<string, PlanStatus>>(new Map());
  const [indexTitleOverrides, setIndexTitleOverrides] = useState<ReadonlyMap<string, string>>(new Map());
  const [doneExpanded, setDoneExpanded] = useState(false);
  const [cancelledOpen, setCancelledOpen] = useState(false);

  const scopedIndex = useMemo(
    () => orderPlanIndex(filterPlanIndex(planIndex ?? [], {
      mode,
      planId,
      teamId,
      orchestratorTabId,
      tabs,
      workspaceId,
      workspaces,
      teams,
      projectPath,
      plans: [],
    })),
    [planIndex, mode, planId, teamId, orchestratorTabId, tabs, workspaceId, projectPath, workspaces, teams],
  );
  const activeIndex = useMemo(
    () => scopedIndex.filter((entry) => (indexStatusOverrides.get(entry.plan_id) ?? entry.status) === "active"),
    [scopedIndex, indexStatusOverrides],
  );
  const defaultIndexPlanId = useMemo(
    () => selectIndexPlanId(activeIndex, { mode, planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams, plans: [] }),
    [activeIndex, mode, planId, teamId, orchestratorTabId, tabs, workspaceId, workspaces, teams],
  );
  const selectedIndexPlan = useMemo(() => {
    const manual = manualPlanId
      ? scopedIndex.find((entry) => entry.plan_id === manualPlanId && (indexStatusOverrides.get(entry.plan_id) ?? entry.status) === "archived")
      : undefined;
    const selectedId = manual?.plan_id ??
      (manualPlanId && activeIndex.some((entry) => entry.plan_id === manualPlanId) ? manualPlanId : defaultIndexPlanId);
    return scopedIndex.find((entry) => entry.plan_id === selectedId) ?? null;
  }, [scopedIndex, manualPlanId, activeIndex, defaultIndexPlanId, indexStatusOverrides]);
  const indexSwitcherOptions = useMemo<PlanSwitcherOption[]>(
    () => scopedIndex.map((entry) => ({
      id: entry.plan_id,
      title: indexTitleOverrides.get(entry.plan_id) ?? (entry.title.trim() || "Untitled plan"),
      archived: (indexStatusOverrides.get(entry.plan_id) ?? entry.status) === "archived",
    })),
    [scopedIndex, indexStatusOverrides, indexTitleOverrides],
  );
  const onIndexAction = useCallback((action: PlanKebabAction) => {
    if (action.kind === "rename") {
      const currentEntry = scopedIndex.find((entry) => entry.plan_id === action.planId);
      const current = indexTitleOverrides.get(action.planId) ?? currentEntry?.title.trim() ?? "Untitled plan";
      const next = window.prompt("Plan name:", current);
      if (!next || next.trim() === current) return;
      const title = next.trim();
      setIndexTitleOverrides((previous) => new Map(previous).set(action.planId, title));
      if (api) renamePlan(api, action.planId, title).catch(() => setIndexTitleOverrides((previous) => {
        const nextTitles = new Map(previous);
        nextTitles.delete(action.planId);
        return nextTitles;
      }));
      return;
    }
    const status: PlanStatus = action.kind === "archive" ? "archived" : "active";
    setIndexStatusOverrides((previous) => new Map(previous).set(action.planId, status));
    const mutate = action.kind === "archive" ? archivePlan : unarchivePlan;
    if (api) mutate(api, action.planId).catch(() => setIndexStatusOverrides((previous) => {
      const nextStatuses = new Map(previous);
      nextStatuses.delete(action.planId);
      return nextStatuses;
    }));
  }, [api, scopedIndex, indexTitleOverrides]);

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

  const taskBoard = usePlanTaskBoard(api, pagedMode ? selectedIndexPlan?.plan_id ?? null : null);

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

  const openTask = useCallback((task: TaskSelection) => {
    if (!selectedPlan) return;
    onOpenPlanTarget?.({ planId: selectedPlan.id, taskId: task.id, teamId: selectedPlan.team_id });
  }, [selectedPlan, onOpenPlanTarget]);

  const showsData = planFetchShowsData(loading, error ?? null, plans.length);

  if (pagedMode) {
    const counts = taskBoard.data?.counts;
    const onOpenSummaryTask = (task: PlanTaskBoardSummary) => {
      if (!selectedIndexPlan) return;
      onOpenPlanTarget?.({ planId: selectedIndexPlan.plan_id, taskId: task.id, teamId: selectedIndexPlan.team_id });
    };
    const retryIndex = () => { void onRetry?.(); };
    const hasIndexPlans = scopedIndex.length > 0;
    return (
      <div data-testid="plan-board-view" className="plan-board-view">
        {(loading || (selectedIndexPlan && taskBoard.loading && !taskBoard.data)) && (
          <Loader variant="fullscreen" label="Loading tasks" data-testid="plan-board-loading" />
        )}
        {error && !hasIndexPlans && !loading && (
          <div data-testid="plan-board-error" className="plan-fetch-state plan-fetch-error flex flex-col items-center gap-2 px-4 text-center text-sm">
            <InlineErrorNotice error={error} operation="loading plans" />
            <button type="button" data-testid="plan-board-retry" onClick={retryIndex}>Retry</button>
          </div>
        )}
        {!loading && !error && !selectedIndexPlan && (
          <p data-testid="plan-board-empty" className="plan-board-state">No active plans in this workspace.</p>
        )}
        {taskBoard.error && !taskBoard.data && selectedIndexPlan && !taskBoard.loading && (
          <div data-testid="plan-board-error" className="plan-fetch-state plan-fetch-error flex flex-col items-center gap-2 px-4 text-center text-sm">
            <InlineErrorNotice error={taskBoard.error} operation="loading tasks" />
            <button type="button" data-testid="plan-board-retry" onClick={() => void taskBoard.refetch()}>Retry</button>
          </div>
        )}
        {taskBoard.reconnecting && taskBoard.data && (
          <PlanFetchReconnectingBanner reconnecting itemCount={1} testId="plan-board-reconnecting" className="plan-board-state px-3 py-1" />
        )}
        {taskBoard.data && selectedIndexPlan && (
          <div data-testid="plan-board-single" className="plan-board-single">
            {!teamId && (
              <PlanSwitcher
                plans={indexSwitcherOptions}
                selectedId={selectedIndexPlan.plan_id}
                onSelect={selectPlan}
                onAction={onIndexAction}
              />
            )}
            <section data-testid="plan-board-plan" data-plan-id={selectedIndexPlan.plan_id} className="plan-board-plan">
              <div data-testid="plan-board-columns" className="plan-board-columns" role="list">
                {BOARD_COLUMN_DEFS.map((column) => {
                  const page = taskBoard.data?.columns?.[column.key];
                  const count = counts?.[column.key] ?? 0;
                  return (
                    <SummaryBoardColumn
                      key={column.key}
                      columnKey={column.key}
                      label={column.label}
                      count={count}
                      items={page?.items ?? []}
                      expanded={doneExpanded}
                      loading={taskBoard.loadingStatus(column.key)}
                      error={taskBoard.errorForStatus(column.key)}
                      canLoadMore={Boolean(page?.next_cursor)}
                      onToggleExpanded={() => setDoneExpanded((value) => !value)}
                      onLoadMore={() => void taskBoard.loadNextPage(column.key)}
                      onOpenTask={onOpenSummaryTask}
                      planId={selectedIndexPlan.plan_id}
                      tabs={tabs}
                      onDeleteTask={taskActions.onDeleteTask}
                    />
                  );
                })}
              </div>
              {(counts?.cancelled ?? 0) > 0 && (
                <>
                  <button
                    type="button"
                    data-testid="plan-board-cancelled-note"
                    className="plan-board-cancelled-note plan-board-cancelled-trigger"
                    onClick={() => {
                      setCancelledOpen((open) => !open);
                      if (!cancelledOpen && !taskBoard.data?.columns?.cancelled) void taskBoard.loadNextPage("cancelled");
                    }}
                    aria-expanded={cancelledOpen}
                  >
                    {counts?.cancelled} cancelled task{counts?.cancelled === 1 ? "" : "s"} hidden from board
                  </button>
                  {cancelledOpen && (
                    <div data-testid="plan-board-cancelled-panel" className="plan-board-cancelled-panel">
                      {(taskBoard.data.columns?.cancelled?.items ?? []).map((task) => (
                        <SummaryTaskCard key={task.id} task={task} planId={selectedIndexPlan.plan_id} tabs={tabs} onOpen={() => onOpenSummaryTask(task)} onDeleteTask={taskActions.onDeleteTask} />
                      ))}
                      {taskBoard.loadingStatus("cancelled") && <Loader variant="compact" label="Loading cancelled tasks" />}
                      {taskBoard.errorForStatus("cancelled") && (
                        <div className="flex flex-col items-center gap-2 px-2 py-1">
                          <InlineErrorNotice error={taskBoard.errorForStatus("cancelled")} operation="loading cancelled tasks" />
                          <button type="button" onClick={() => void taskBoard.loadNextPage("cancelled")}>Retry</button>
                        </div>
                      )}
                      {taskBoard.data.columns?.cancelled?.next_cursor && (
                        <button type="button" disabled={taskBoard.loadingStatus("cancelled")} onClick={() => void taskBoard.loadNextPage("cancelled")}>
                          Load more cancelled tasks
                        </button>
                      )}
                    </div>
                  )}
                </>
              )}
            </section>
          </div>
        )}
      </div>
    );
  }

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
