import type { BoardPlanSummary, BoardSpecSummary } from "./boardModel";

export function BoardSpecSummaryCard({ spec }: { spec: BoardSpecSummary }) {
  return (
    <section
      data-testid="plan-board-spec-summary"
      data-spec-id={spec.specId}
      className="plan-board-entity-summary plan-board-spec-summary"
    >
      <p className="plan-board-entity-kind">Spec</p>
      <h3 data-testid="plan-board-spec-title" className="plan-board-entity-title">
        {spec.title}
      </h3>
      {spec.sectionCount > 0 && (
        <p data-testid="plan-board-spec-sections" className="plan-board-entity-meta">
          {spec.sectionCount} section{spec.sectionCount === 1 ? "" : "s"}
          {spec.leadSection ? ` · starts with ${spec.leadSection}` : ""}
        </p>
      )}
    </section>
  );
}

export function BoardPlanSummaryCard({ summary }: { summary: BoardPlanSummary }) {
  const progress =
    summary.total > 0 ? `${summary.done}/${summary.total} done` : "No active tasks";
  return (
    <section data-testid="plan-board-plan-summary" className="plan-board-entity-summary plan-board-plan-summary">
      <p className="plan-board-entity-kind">Plan</p>
      <h3 data-testid="plan-board-plan-title" className="plan-board-entity-title">
        {summary.title}
      </h3>
      <p data-testid="plan-board-plan-meta" className="plan-board-entity-meta">
        {summary.ownerLabel} · {progress}
        {summary.sectionCount > 0 ? ` · ${summary.sectionCount} sections` : ""}
      </p>
      {summary.goalVector && (
        <p data-testid="plan-board-plan-goal" className="plan-board-entity-body">
          {summary.goalVector}
        </p>
      )}
      {summary.focus && (
        <p data-testid="plan-board-plan-focus" className="plan-board-entity-focus">
          Focus: {summary.focus}
        </p>
      )}
    </section>
  );
}
