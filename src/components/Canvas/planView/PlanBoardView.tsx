import { useCallback, useEffect, useMemo, useState } from "react";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import { fetchSpecSections } from "../plansApi";
import { fixtureSpecSectionsByPlan } from "../perfFixture";
import type { Plan, Section, Task } from "../types";
import {
  boundDoneColumnCards,
  buildBoardModels,
  DONE_COLUMN_COLLAPSED_LIMIT,
  filterPlansForBoard,
  type BoardColumnKey,
  type BoardPlanModel,
} from "./boardModel";
import { BoardTaskCard } from "./BoardTaskCard";
import { PlanContextHeader } from "./PlanContextHeader";
import { PlanDetailOverlay, type PlanDetailKind } from "./PlanDetailOverlay";

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

  const plansById = useMemo(() => new Map(filteredPlans.map((p) => [p.id, p])), [filteredPlans]);

  const fixtureSpecSections = useMemo(() => fixtureSpecSectionsByPlan(filteredPlans), [filteredPlans]);
  const [fetchedSpecSections, setFetchedSpecSections] = useState<Map<string, Section[]>>(new Map());
  const [detail, setDetail] = useState<BoardDetail | null>(null);

  useEffect(() => {
    if (!api) return;
    let active = true;
    const targets = filteredPlans.filter((p) => p.primary_spec_id && !fixtureSpecSections.has(p.id));
    if (!targets.length) return;
    void Promise.all(
      targets.map((p) =>
        fetchSpecSections(api, p.primary_spec_id as string)
          .then((sections) => [p.id, sections] as const)
          .catch(() => [p.id, [] as Section[]] as const),
      ),
    ).then((pairs) => {
      if (!active) return;
      setFetchedSpecSections(new Map(pairs));
    });
    return () => {
      active = false;
    };
  }, [api, filteredPlans, fixtureSpecSections]);

  const specSectionsByPlan = useMemo(() => {
    const merged = new Map<string, Section[]>(fetchedSpecSections);
    for (const [id, sections] of fixtureSpecSections) merged.set(id, sections);
    return merged;
  }, [fetchedSpecSections, fixtureSpecSections]);

  const boardModels = useMemo(
    () => buildBoardModels(filteredPlans, panes, specSectionsByPlan),
    [filteredPlans, panes, specSectionsByPlan],
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
      {!loading && !error && boardModels.length === 0 && (
        <p data-testid="plan-board-empty" className="plan-board-state">
          No active plans in this workspace.
        </p>
      )}
      {!loading && !error && boardModels.length > 0 && (
        <div data-testid="plan-board-stack" className="plan-board-stack">
          {boardModels.map((model) => {
            const plan = plansById.get(model.planId);
            if (!plan) return null;
            return (
              <BoardPlanBoard
                key={model.planId}
                model={model}
                plan={plan}
                specSections={specSectionsByPlan.get(model.planId) ?? []}
                onOpenDetail={setDetail}
              />
            );
          })}
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
