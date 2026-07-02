import { useState } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import type { BoardPlanSummary, BoardSpecSummary } from "./boardModel";

const GOAL_TRUNCATE = 120;

export interface PlanContextHeaderProps {
  summary: BoardPlanSummary;
  spec: BoardSpecSummary | null;
  onOpenPlan: () => void;
  onOpenSpec?: () => void;
}

function progressLabel(done: number, total: number): string {
  if (total <= 0) return "No active tasks";
  return `${done}/${total} done`;
}

export function PlanContextHeader({ summary, spec, onOpenPlan, onOpenSpec }: PlanContextHeaderProps) {
  const [goalExpanded, setGoalExpanded] = useState(false);
  const goal = summary.goalVector?.trim() ?? "";
  const goalLong = goal.length > GOAL_TRUNCATE;
  const goalVisible = goal && (goalExpanded || !goalLong) ? goal : goal.slice(0, GOAL_TRUNCATE);

  return (
    <header data-testid="plan-context-header" className="plan-context-header">
      <div className="plan-context-header-primary">
        <button
          type="button"
          data-testid="plan-board-plan-title"
          className="plan-context-header-plan-trigger"
          onClick={onOpenPlan}
          title="Open plan detail"
        >
          {summary.title}
        </button>
        <span data-testid="plan-board-plan-meta" className="plan-context-header-meta">
          {summary.ownerLabel} · {progressLabel(summary.done, summary.total)}
          {summary.sectionCount > 0 ? ` · ${summary.sectionCount} sections` : ""}
        </span>
        {spec && onOpenSpec && (
          <button
            type="button"
            data-testid="plan-board-spec-title"
            className="plan-context-header-spec-trigger"
            onClick={onOpenSpec}
            title="Open spec detail"
          >
            <span data-testid="plan-board-spec-sections" className="plan-context-header-spec-meta">
              {spec.title}
              {spec.sectionCount > 0
                ? ` · ${spec.sectionCount} section${spec.sectionCount === 1 ? "" : "s"}`
                : ""}
            </span>
          </button>
        )}
      </div>
      {(goal || summary.focus) && (
        <div className="plan-context-header-secondary">
          {goal && (
            <p data-testid="plan-board-plan-goal" className="plan-context-header-goal">
              <span className="plan-context-header-goal-text">{goalVisible}{goalLong && !goalExpanded ? "…" : ""}</span>
              {goalLong && (
                <button
                  type="button"
                  className="plan-context-header-goal-toggle"
                  aria-expanded={goalExpanded}
                  onClick={() => setGoalExpanded((v) => !v)}
                >
                  {goalExpanded ? (
                    <>
                      <ChevronUp size={11} aria-hidden />
                      Less
                    </>
                  ) : (
                    <>
                      <ChevronDown size={11} aria-hidden />
                      More
                    </>
                  )}
                </button>
              )}
            </p>
          )}
          {summary.focus && (
            <p data-testid="plan-board-plan-focus" className="plan-context-header-focus">
              {summary.focus}
            </p>
          )}
        </div>
      )}
    </header>
  );
}
