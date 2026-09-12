import type { ReactNode } from "react";
import type { PlanningState } from "../types";
import type { BoardPlanSummary } from "./boardModel";
import { PairAffordance } from "../../shared/PairAffordance";

export interface PlanBoardStatusHeaderProps {
  summary: BoardPlanSummary;
  onOpenPlan: () => void;
  onOpenTab?: (tabId: string) => void;
  /** Replaces the bare done/total count; Mission Control passes its fuller reading. */
  progress?: ReactNode;
}

const PLANNING_STATE_LABEL: Record<PlanningState, string> = {
  drafting: "Drafting",
  structure_ready: "Structure ready",
  approved: "Approved",
};

function planningStateLabel(state: PlanningState | null | undefined): string | null {
  return state ? PLANNING_STATE_LABEL[state] : null;
}

function progressLabel(done: number, total: number): string {
  return `${done}/${total} done`;
}

export function PlanBoardStatusHeader({
  summary,
  onOpenPlan,
  onOpenTab,
  progress,
}: PlanBoardStatusHeaderProps) {
  const stateLabel = planningStateLabel(summary.planningState);
  const plannerLabel = summary.plannerLabel ?? "No planner";

  return (
    <header data-testid="plan-board-status" className="plan-board-status">
      <div className="plan-board-status-topline">
        <div className="plan-board-status-chips">
          {stateLabel && (
            <span
              data-testid="plan-board-approval-state"
              className={`plan-context-chip plan-state-chip plan-state-${summary.planningState}`}
            >
              {stateLabel}
            </span>
          )}
          <PairAffordance
            layout="chips"
            data-testid="plan-board-peer-pair"
            orchestrator={
              <PeerChip
                role="orchestrator"
                label={summary.ownerLabel}
                tabId={summary.ownerTabId}
                onOpenTab={onOpenTab}
              />
            }
            planner={
              <PeerChip
                role="planner"
                label={plannerLabel}
                tabId={summary.plannerTabId}
                onOpenTab={onOpenTab}
              />
            }
          />
        </div>
        <span data-testid="plan-board-done-count" className="plan-board-done-count">
          {progress ?? progressLabel(summary.done, summary.total)}
        </span>
      </div>
      <div className="plan-board-status-title-row">
        <button
          type="button"
          data-testid="plan-board-plan-title"
          className="plan-board-status-title"
          onClick={onOpenPlan}
          title="Open plan detail"
        >
          {summary.title}
        </button>
      </div>
    </header>
  );
}

interface PeerChipProps {
  role: "planner" | "orchestrator";
  label: string;
  tabId: string | null;
  onOpenTab?: (tabId: string) => void;
}

function PeerChip({ role, label, tabId, onOpenTab }: PeerChipProps) {
  const roleLabel = role === "planner" ? "Planner" : "Orchestrator";
  const clickable = Boolean(tabId && onOpenTab);
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
        data-testid={`plan-board-peer-${role}`}
        className={`plan-context-chip plan-peer-chip plan-peer-chip-${role}`}
        onClick={() => onOpenTab!(tabId!)}
        title={`Open ${roleLabel} tab`}
      >
        {content}
      </button>
    );
  }

  return (
    <span
      data-testid={`plan-board-peer-${role}`}
      className={`plan-context-chip plan-peer-chip plan-peer-chip-${role} plan-peer-chip-static`}
    >
      {content}
    </span>
  );
}
