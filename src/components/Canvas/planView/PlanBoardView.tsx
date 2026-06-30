import { useEffect, useMemo, useState } from "react";
import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";
import { fetchSpecSections } from "../plansApi";
import { fixtureSpecSectionsByPlan } from "../perfFixture";
import type { Section } from "../types";
import { buildBoardModels, filterPlansForBoard, type BoardPlanModel } from "./boardModel";
import { BoardPlanSummaryCard, BoardSpecSummaryCard } from "./BoardEntitySummary";
import { BoardTaskCard } from "./BoardTaskCard";

function BoardPlanBoard({ model }: { model: BoardPlanModel }) {
  return (
    <section data-testid="plan-board-plan" data-plan-id={model.planId} className="plan-board-plan">
      <div data-testid="plan-board-summaries" className="plan-board-summaries">
        {model.spec && <BoardSpecSummaryCard spec={model.spec} />}
        <BoardPlanSummaryCard summary={model.planSummary} />
      </div>
      <div data-testid="plan-board-columns" className="plan-board-columns" role="list">
        {model.columns.map((column) => (
          <div key={column.key} data-testid="plan-board-column" data-column={column.key} className="plan-board-column" role="listitem">
            <header className="plan-board-column-head">
              <span data-testid="plan-board-column-label">{column.label}</span>
              <span data-testid="plan-board-column-count" className="plan-board-column-count">
                {column.cards.length}
              </span>
            </header>
            <div className="plan-board-column-body">
              {column.cards.map((card) => (
                <BoardTaskCard key={card.task.id} card={card} />
              ))}
            </div>
          </div>
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

export function PlanBoardView({ mode, plans, loading = false, error = null, planId, orchestratorPaneId, panes = {}, api }: PlanViewSurfaceProps) {
  const filteredPlans = useMemo(
    () => filterPlansForBoard(plans, mode, { planId, orchestratorPaneId, panes }),
    [plans, mode, planId, orchestratorPaneId, panes],
  );

  const fixtureSpecSections = useMemo(() => fixtureSpecSectionsByPlan(filteredPlans), [filteredPlans]);
  const [fetchedSpecSections, setFetchedSpecSections] = useState<Map<string, Section[]>>(new Map());

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

  return (
    <div data-testid="plan-board-view" className="plan-board-view" style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
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
          {boardModels.map((model) => (
            <BoardPlanBoard key={model.planId} model={model} />
          ))}
        </div>
      )}
    </div>
  );
}
