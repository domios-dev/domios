import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { fetchSpecSections } from "../plansApi";
import { fixtureSpecSectionsByPlan } from "../perfFixture";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import type { Plan, Section, Task } from "../types";
import {
  buildBoardPlanModel,
  filterPlansForBoard,
  orderPlansByActivity,
  selectDefaultPlanId,
} from "./boardModel";
import { buildPlanListRows, type PlanListRow } from "./buildPlanListTree";
import { cancelledTaskCount } from "./cancelledTasks";
import { PlanContextHeader } from "./PlanContextHeader";
import { PlanDetailOverlay, type PlanDetailKind } from "./PlanDetailOverlay";
import { PlanFetchErrorState, PlanFetchReconnectingBanner } from "./PlanFetchErrorState";
import { planFetchShowsData } from "./planViewFetchGate";
import { PlanListFilterBar } from "./PlanListFilterBar";
import { PlanListRowView } from "./PlanListRowView";
import { PlanSwitcher } from "./PlanSwitcher";
import { usePlanSwitcherActions } from "./usePlanSwitcherActions";
import { ActiveWorkStrip } from "./ActiveWorkStrip";
import { enrichSwitcherOptions, selectActiveWork, type ActiveWorkEntry } from "./activeWork";
import { activeWorkEntriesForStrip } from "./activeWorkStripVisibility";
import {
  boundDoneFilterVisibleRows,
  buildVisiblePlanListRows,
  resolveSectionExpanded,
  type PlanListStatusFilter,
} from "./planListModel";
import { EMPTY_NAV_STACK, navCanGoBack, navPop, navPush, type NavEntry, type NavStack } from "./navStack";

const SELECTED_PLAN_STORAGE_KEY = "codeterm.list.selectedPlanId";

function readStoredPlanId(): string | null {
  try {
    return window.sessionStorage.getItem(SELECTED_PLAN_STORAGE_KEY);
  } catch {
    return null;
  }
}

type ListDetail =
  | { kind: "task"; task: Task; plan: Plan }
  | { kind: "plan"; plan: Plan; specSections: Section[] }
  | { kind: "spec"; plan: Plan; specSections: Section[] };

function findTask(plan: Plan, taskId: string): Task | undefined {
  return plan.tasks.find((t) => t.id === taskId);
}

function detailToEntry(detail: ListDetail, scroll?: number): NavEntry {
  if (detail.kind === "task") {
    return { surface: "task", id: detail.task.id, planId: detail.plan.id, scroll };
  }
  return { surface: detail.kind, id: detail.plan.id, planId: detail.plan.id, scroll };
}

function entryToDetail(
  entry: NavEntry,
  plans: Plan[],
  specSections: Section[],
): ListDetail | null {
  const plan = plans.find((p) => p.id === entry.planId);
  if (!plan) return null;
  if (entry.surface === "task") {
    const task = findTask(plan, entry.id);
    if (!task) return null;
    return { kind: "task", task, plan };
  }
  return { kind: entry.surface, plan, specSections };
}

export function PlanListView({
  mode,
  plans,
  loading = false,
  reconnecting = false,
  error = null,
  onRetry,
  planId,
  teamId,
  orchestratorPaneId,
  panes = {},
  workspaceId,
  workspaces,
  teams,
  onOpenPane,
  api,
  onClose,
}: PlanViewSurfaceProps) {
  const { effectivePlans, switcherOptions, handleAction } = usePlanSwitcherActions(plans, api);

  const activeWork = useMemo(
    () => selectActiveWork(effectivePlans, panes),
    [effectivePlans, panes],
  );
  const switcherWithInflight = useMemo(
    () => enrichSwitcherOptions(switcherOptions, activeWork),
    [switcherOptions, activeWork],
  );
  const [manualPlanId, setManualPlanId] = useState<string | null>(readStoredPlanId);

  const filteredPlans = useMemo(
    () => {
      const active = filterPlansForBoard(effectivePlans, mode, { planId, teamId, orchestratorPaneId, panes, workspaceId, workspaces, teams });
      if (!manualPlanId || active.some((p) => p.id === manualPlanId)) return active;
      const archivedPlan = plans.find((p) => p.id === manualPlanId && p.status === "archived");
      if (!archivedPlan || (mode === "focus" && planId && planId !== archivedPlan.id)) return active;
      return [...active, archivedPlan];
    },
    [effectivePlans, manualPlanId, mode, planId, teamId, orchestratorPaneId, panes, workspaceId, workspaces, teams, plans],
  );
  const orderedPlans = useMemo(() => orderPlansByActivity(filteredPlans), [filteredPlans]);

  const defaultPlanId = useMemo(
    () => selectDefaultPlanId(orderedPlans, { planId, teamId, orchestratorPaneId, panes, workspaceId, workspaces, teams }),
    [orderedPlans, planId, teamId, orchestratorPaneId, panes, workspaceId, workspaces, teams],
  );

  const [navStack, setNavStack] = useState<NavStack>(EMPTY_NAV_STACK);
  const bodyScrollRef = useRef<HTMLDivElement | null>(null);
  const [restoreScroll, setRestoreScroll] = useState<number | undefined>(undefined);
  const selectedPlanId = useMemo(() => {
    if (manualPlanId && orderedPlans.some((p) => p.id === manualPlanId)) return manualPlanId;
    return defaultPlanId;
  }, [manualPlanId, orderedPlans, defaultPlanId]);

  const selectedPlan = useMemo(
    () => orderedPlans.find((p) => p.id === selectedPlanId) ?? null,
    [orderedPlans, selectedPlanId],
  );

  const planActiveWork = useMemo(
    () => (selectedPlan ? activeWork.byPlanId.get(selectedPlan.id) ?? [] : []),
    [activeWork, selectedPlan],
  );

  const selectPlan = useCallback((id: string) => {
    setManualPlanId(id);
    try {
      window.sessionStorage.setItem(SELECTED_PLAN_STORAGE_KEY, id);
    } catch {
      // Best-effort session persistence; private-mode/quota failures must not block switching.
    }
  }, []);

  const fixtureSpecSections = useMemo(
    () => (selectedPlan ? fixtureSpecSectionsByPlan([selectedPlan]) : new Map<string, Section[]>()),
    [selectedPlan],
  );
  const [fetchedSpecSections, setFetchedSpecSections] = useState<Map<string, Section[]>>(new Map());
  const [statusFilter, setStatusFilter] = useState<PlanListStatusFilter>("all");
  const [doneFilterExpanded, setDoneFilterExpanded] = useState(false);
  const [textQuery, setTextQuery] = useState("");
  const [expansionOverrides, setExpansionOverrides] = useState<Map<string, boolean>>(new Map());
  const [detail, setDetail] = useState<ListDetail | null>(null);

  useEffect(() => {
    setNavStack(EMPTY_NAV_STACK);
    setDetail(null);
  }, [selectedPlanId]);

  useEffect(() => {
    setDoneFilterExpanded(false);
  }, [statusFilter]);

  useEffect(() => {
    if (!api || !selectedPlan?.primary_spec_id) return;
    if (fixtureSpecSections.has(selectedPlan.id)) return;
    let active = true;
    const targetPlanId = selectedPlan.id;
    const specId = selectedPlan.primary_spec_id;
    void fetchSpecSections(api, specId)
      .then((sections) => (active ? ([targetPlanId, sections] as const) : null))
      .catch(() => (active ? ([targetPlanId, [] as Section[]] as const) : null))
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
    () => (selectedPlan ? buildBoardPlanModel(selectedPlan, panes, selectedSpecSections) : null),
    [selectedPlan, panes, selectedSpecSections],
  );

  const cancelledCount = useMemo(
    () => (selectedPlan ? cancelledTaskCount(selectedPlan) : 0),
    [selectedPlan],
  );

  const rows = useMemo(
    () => (selectedPlan ? buildPlanListRows(selectedPlan, { panes }) : []),
    [selectedPlan, panes],
  );

  const visibleRows = useMemo(() => {
    const base = buildVisiblePlanListRows(rows, { statusFilter, textQuery, expansionOverrides });
    if (statusFilter !== "done") return base;
    return boundDoneFilterVisibleRows(base, doneFilterExpanded);
  }, [rows, statusFilter, textQuery, expansionOverrides, doneFilterExpanded]);

  const stripEntries = useMemo(
    () => activeWorkEntriesForStrip(planActiveWork, visibleRows),
    [planActiveWork, visibleRows],
  );

  const toggleSection = useCallback((sectionId: string, sectionRow?: PlanListRow) => {
    setExpansionOverrides((prev) => {
      const next = new Map(prev);
      const current = prev.get(sectionId) ?? (sectionRow ? resolveSectionExpanded(sectionRow, prev) : true);
      next.set(sectionId, !current);
      return next;
    });
  }, []);

  const openRow = useCallback(
    (row: PlanListRow) => {
      if (!selectedPlan || !row.taskId) return;
      const task = findTask(selectedPlan, row.taskId);
      if (task) setDetail({ kind: "task", task, plan: selectedPlan });
    },
    [selectedPlan],
  );
  const openActiveWork = useCallback(
    (entry: ActiveWorkEntry) => {
      const plan = orderedPlans.find((p) => p.id === entry.planId);
      if (!plan) return;
      setDetail({ kind: "task", task: entry.task, plan });
    },
    [orderedPlans],
  );
  const openBelonging = useCallback(() => {
    if (selectedPlan) setDetail({ kind: "plan", plan: selectedPlan, specSections: selectedSpecSections });
  }, [selectedPlan, selectedSpecSections]);

  const closeDetail = useCallback(() => {
    setDetail(null);
    setNavStack(EMPTY_NAV_STACK);
  }, []);

  const pushCurrentDetail = useCallback((next: ListDetail) => {
    if (detail) {
      const scroll = bodyScrollRef.current?.scrollTop ?? 0;
      setNavStack((prev) => navPush(prev, detailToEntry(detail, scroll)));
    }
    setRestoreScroll(undefined);
    setDetail(next);
  }, [detail]);

  const openDetailTask = useCallback((taskId: string) => {
    if (!selectedPlan) return;
    const task = findTask(selectedPlan, taskId);
    if (task) pushCurrentDetail({ kind: "task", task, plan: selectedPlan });
  }, [selectedPlan, pushCurrentDetail]);

  const openDetailSection = useCallback((args: { variant: "plan" | "spec" }) => {
    if (!selectedPlan) return;
    const next: ListDetail = args.variant === "spec"
      ? { kind: "spec", plan: selectedPlan, specSections: selectedSpecSections }
      : { kind: "plan", plan: selectedPlan, specSections: selectedSpecSections };
    pushCurrentDetail(next);
  }, [selectedPlan, selectedSpecSections, pushCurrentDetail]);

  const handleBack = useCallback(() => {
    const { stack, entry } = navPop(navStack);
    if (!entry) return;
    const restored = entryToDetail(entry, orderedPlans, selectedSpecSections);
    if (!restored) return;
    setNavStack(stack);
    setRestoreScroll(entry.scroll);
    setDetail(restored);
  }, [navStack, orderedPlans, selectedSpecSections]);
  const overlayKind: PlanDetailKind | null = detail?.kind ?? null;
  const now = Date.now();
  const showsData = planFetchShowsData(loading, error ?? null, plans.length);

  return (
    <div
      data-testid="plan-list-view"
      className="relative flex h-full w-full flex-col overflow-hidden"
      style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}
    >
      {onClose && (
        <button
          type="button"
          data-testid="plan-list-close"
          onClick={onClose}
          className="absolute top-2 right-2 z-20 flex h-7 w-7 items-center justify-center rounded border cursor-pointer"
          style={{ background: "var(--ct-bg)", borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
          title="Close plan view"
        >
          <X size={14} />
        </button>
      )}
      <PlanFetchErrorState
        loading={loading}
        error={error}
        itemCount={plans.length}
        onRetry={onRetry}
        testId="plan-list"
        className="plan-list-state"
      />
      <PlanFetchReconnectingBanner
        reconnecting={reconnecting}
        itemCount={plans.length}
        testId="plan-list-reconnecting"
        className="absolute top-2 left-3 z-10"
      />
      {showsData && !selectedPlan && (
        <div
          data-testid="plan-list-empty"
          className="flex flex-1 items-center justify-center px-6 text-center text-sm"
          style={{ opacity: "var(--ct-text-secondary-opacity)" }}
        >
          No active plans to list.
        </div>
      )}
      {showsData && selectedPlan && boardModel && (
        <>
          <div className="plan-list-chrome">
            {!teamId && (
              <PlanSwitcher
                plans={switcherWithInflight}
                selectedId={selectedPlan.id}
                onSelect={selectPlan}
                onAction={handleAction}
              />
            )}
            <PlanContextHeader
              summary={boardModel.planSummary}
              spec={boardModel.spec}
              onOpenPlan={() => setDetail({ kind: "plan", plan: selectedPlan, specSections: selectedSpecSections })}
              onOpenSpec={
                boardModel.spec
                  ? () => setDetail({ kind: "spec", plan: selectedPlan, specSections: selectedSpecSections })
                  : undefined
              }
              onOpenPane={onOpenPane}
              planId={selectedPlan.id}
              plan={selectedPlan}
              api={api}
            />
            <PlanListFilterBar
              statusFilter={statusFilter}
              textQuery={textQuery}
              cancelledCount={cancelledCount}
              onStatusFilter={setStatusFilter}
              onTextQuery={setTextQuery}
            />
            <ActiveWorkStrip entries={stripEntries} onOpenTask={openActiveWork} />
          </div>
          <div data-testid="plan-list-scroll" className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden">
            {visibleRows.map((entry) => {
              const sectionRow = entry.type === "row" ? entry.row : entry.type === "done-rollup" ? entry.sectionRow : undefined;
              const sectionExpanded =
                entry.type === "row" && sectionRow && (sectionRow.kind === "plan-section" || sectionRow.kind === "unsectioned")
                  ? resolveSectionExpanded(sectionRow, expansionOverrides)
                  : undefined;
              const rowKey =
                entry.type === "row"
                  ? entry.row.id
                  : entry.type === "done-rollup"
                    ? `rollup::${entry.sectionRow.id}`
                    : "done-filter-toggle";
              return (
                <PlanListRowView
                  key={rowKey}
                  entry={entry}
                  now={now}
                  sectionExpanded={sectionExpanded}
                  onToggleSection={(id) => {
                    const targetRow = rows.find((r) => r.id === id);
                    toggleSection(id, targetRow);
                  }}
                  onToggleDoneFilter={() => setDoneFilterExpanded((v) => !v)}
                  onOpenRow={openRow}
                  onOpenBelonging={openBelonging}
                />
              );
            })}
          </div>
        </>
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
          panes={panes}
          onOpenPane={onOpenPane}
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
