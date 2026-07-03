import type { PlanningState } from "../types";
import type { BoardPlanSummary, BoardSpecSummary } from "./boardModel";

export interface PlanContextHeaderProps {
  summary: BoardPlanSummary;
  spec: BoardSpecSummary | null;
  onOpenPlan: () => void;
  onOpenSpec?: () => void;
  onOpenPane?: (paneId: string) => void;
}

function progressLabel(done: number, total: number): string {
  if (total <= 0) return "No active tasks";
  return `${done}/${total} done`;
}

const PLANNING_STATE_LABEL: Record<PlanningState, string> = {
  drafting: "Drafting",
  structure_ready: "Structure ready",
  approved: "Approved",
};

export function PlanContextHeader({
  summary,
  spec,
  onOpenPlan,
  onOpenSpec,
  onOpenPane,
}: PlanContextHeaderProps) {
  const goal = summary.goalVector?.trim() ?? "";
  const stateLabel = summary.planningState ? PLANNING_STATE_LABEL[summary.planningState] : null;

  return (
    <header data-testid="plan-context-header" className="plan-context-header">
      <div className="plan-context-header-chips" data-testid="plan-context-header-chips">
        {stateLabel && (
          <span
            data-testid="plan-state-chip"
            className={`plan-context-chip plan-state-chip plan-state-${summary.planningState}`}
            title={
              summary.executionUnlocked
                ? "Execution tasks unlocked"
                : "Execution tasks locked until approved"
            }
          >
            {stateLabel}
          </span>
        )}
        <PeerChip
          role="planner"
          label={summary.plannerLabel ?? "No planner"}
          paneId={summary.plannerPaneId}
          onOpenPane={onOpenPane}
        />
        <PeerChip
          role="orchestrator"
          label={summary.ownerLabel}
          paneId={summary.ownerPaneId}
          onOpenPane={onOpenPane}
        />
      </div>
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
              <span className="plan-context-header-goal-text">{goal}</span>
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

interface PeerChipProps {
  role: "planner" | "orchestrator";
  label: string;
  paneId: string | null;
  onOpenPane?: (paneId: string) => void;
}

function PeerChip({ role, label, paneId, onOpenPane }: PeerChipProps) {
  const roleLabel = role === "planner" ? "Planner" : "Orchestrator";
  const clickable = Boolean(paneId && onOpenPane);
  const content = (
    <>
      <span className="plan-peer-chip-role">{roleLabel}</span>
      <span className="plan-peer-chip-name">{label}</span>
    </>
  );
  if (clickable) {
    return (
      <button
        type="button"
        data-testid={`plan-peer-chip-${role}`}
        className={`plan-context-chip plan-peer-chip plan-peer-chip-${role}`}
        onClick={() => onOpenPane!(paneId!)}
        title={`Open ${roleLabel} pane`}
      >
        {content}
      </button>
    );
  }
  return (
    <span
      data-testid={`plan-peer-chip-${role}`}
      className={`plan-context-chip plan-peer-chip plan-peer-chip-${role} plan-peer-chip-static`}
    >
      {content}
    </span>
  );
}
