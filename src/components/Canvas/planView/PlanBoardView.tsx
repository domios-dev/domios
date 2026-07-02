import { useCallback, useEffect, useMemo, useState } from "react";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import { fetchSpecSections } from "../plansApi";
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
import { BoardTaskCard } from "./BoardTaskCard";
import { PlanContextHeader } from "./PlanContextHeader";
import { PlanSwitcher } from "./PlanSwitcher";
import { PlanDetailOverlay, type PlanDetailKind } from "./PlanDetailOverlay";

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
  | { kind: "plan"; plan: Plan }
  | { kind: "spec"; plan: Plan; specSections: Section[] };

function BoardColumn({
  columnKey,
  label,
  cards,
  doneExpanded,
  onToggleDone,
  onOpenTask,
}: {
  columnKey: BoardColumnKey;
  label: string;
  cards: BoardPlanModel["columns"][number]["cards"];
  doneExpanded: boolean;
  onToggleDone: () => void;
  onOpenTask: (task: Task) => void;
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
          <BoardTaskCard key={card.task.id} card={card} onOpen={() => onOpenTask(card.task)} />
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
  specSections,
  onOpenDetail,
}: {
  model: BoardPlanModel;
  plan: Plan;
  specSections: Section[];
  onOpenDetail: (detail: BoardDetail) => void;
}) {
  const [doneExpanded, setDoneExpanded] = useState(false);

  return (
    <section data-testid="plan-board-plan" data-plan-id={model.planId} className="plan-board-plan">
      <PlanContextHeader
        summary={model.planSummary}
        spec={model.spec}
        onOpenPlan={() => onOpenDetail({ kind: "plan", plan })}
        onOpenSpec={
          model.spec
            ? () => onOpenDetail({ kind: "spec", plan, specSections })
            : undefined
        }
      />
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
          />
        ))}
      </div>
      {model.cancelledCount > 0 && (
        <p data-testid="plan-board-cancelled-note" className="plan-board-cancelled-note">
          {model.cancelledCount} cancelled task{model.cancelledCount === 1 ? "" : "s"} hidden from board
        </p>
      )}
    </section>
  );
}

export function PlanBoardView({
  mode,
  plans,
  loading = false,
  error = null,
  planId,
  orchestratorPaneId,
  panes = {},
  onOpenPane,
  api,
}: PlanViewSurfaceProps) {
  const filteredPlans = useMemo(
    () => filterPlansForBoard(plans, mode, { planId, orchestratorPaneId, panes }),
    [plans, mode, planId, orchestratorPaneId, panes],
  );

  const orderedPlans = useMemo(() => orderPlansByActivity(filteredPlans), [filteredPlans]);

  const defaultPlanId = useMemo(
    () => selectDefaultPlanId(orderedPlans, { planId, orchestratorPaneId, panes }),
    [orderedPlans, planId, orchestratorPaneId, panes],
  );

  const [manualPlanId, setManualPlanId] = useState<string | null>(readStoredPlanId);

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
    () => (selectedPlan ? buildBoardPlanModel(selectedPlan, panes, selectedSpecSections) : null),
    [selectedPlan, panes, selectedSpecSections],
  );

  const switcherOptions = useMemo(
    () => orderedPlans.map((p) => ({ id: p.id, title: p.title.trim() || "Untitled plan" })),
    [orderedPlans],
  );

  const closeDetail = useCallback(() => setDetail(null), []);

  const overlayKind: PlanDetailKind | null = detail?.kind ?? null;

  return (
    <div data-testid="plan-board-view" className="plan-board-view">
      {loading && (
        <p data-testid="plan-board-loading" className="plan-board-state">
          Loading plans…
        </p>
      )}
      {!loading && error && (
        <p data-testid="plan-board-error" className="plan-board-state plan-board-error">
          {error}
        </p>
      )}
      {!loading && !error && !selectedPlan && (
        <p data-testid="plan-board-empty" className="plan-board-state">
          No active plans in this workspace.
        </p>
      )}
      {!loading && !error && selectedPlan && boardModel && (
        <div data-testid="plan-board-single" className="plan-board-single">
          <PlanSwitcher plans={switcherOptions} selectedId={selectedPlan.id} onSelect={selectPlan} />
          <BoardPlanBoard
            model={boardModel}
            plan={selectedPlan}
            specSections={selectedSpecSections}
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
          specSections={detail.kind === "spec" ? detail.specSections : undefined}
          planId={detail.plan.id}
          panes={panes}
          onOpenPane={onOpenPane}
          onClose={closeDetail}
        />
      )}
    </div>
  );
}
