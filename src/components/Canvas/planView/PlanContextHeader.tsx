import { ChevronDown } from "lucide-react";
import type { PlanningState } from "../types";
import type { ApiConfig } from "../../../lib/types";
import type { BoardPlanSummary, BoardSpecSummary } from "./boardModel";
import type { Plan } from "../types";
import { PairAffordance } from "../../shared/PairAffordance";
import { isPlanParked } from "./PlanModeControls";
import { useTouchOptimizedUi } from "../../../hooks/useTouchOptimizedUi";
import { useNarrowPlanHeader, usePlanHeaderCollapse } from "./usePlanHeaderCollapse";

export interface PlanContextHeaderProps {
  summary: BoardPlanSummary;
  spec: BoardSpecSummary | null;
  onOpenPlan: () => void;
  onOpenSpec?: () => void;
  onOpenTab?: (tabId: string) => void;
  planId?: string;
  plan?: Plan;
  api?: ApiConfig;
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

function planningStateLabel(state: PlanningState | null | undefined): string | null {
  return state ? PLANNING_STATE_LABEL[state] : null;
}

export function PlanContextHeader(props: PlanContextHeaderProps) {
  const touch = useTouchOptimizedUi();
  const narrow = useNarrowPlanHeader();
  if (touch || narrow) return <PlanContextCompactHeader {...props} />;
  return <PlanContextWideHeader {...props} />;
}

export function PlanContextCompactHeader(props: PlanContextHeaderProps) {
  const { summary, planId, plan } = props;
  const { collapsed, toggle } = usePlanHeaderCollapse("narrow", true, planId);
  const expanded = !collapsed;
  const stateLabel = planningStateLabel(summary.planningState);
  const parked = isPlanParked(plan);

  return (
    <header
      data-testid="plan-context-header"
      className={`plan-context-header plan-context-header-compact${expanded ? " is-expanded" : ""}`}
    >
      <button
        type="button"
        data-testid="plan-context-compact-bar"
        className="plan-context-compact-bar"
        aria-expanded={expanded}
        aria-controls="plan-context-expanded-panel"
        onClick={toggle}
        title={expanded ? "Collapse plan context" : "Expand plan context"}
      >
        <ChevronDown
          size={16}
          aria-hidden={false}
          aria-label={expanded ? "Collapse plan context" : "Expand plan context"}
          className="plan-context-compact-chevron"
        />
        {stateLabel && (
          <span
            data-testid="plan-state-chip"
            className={`plan-context-chip plan-state-chip plan-state-${summary.planningState}`}
          >
            {stateLabel}
          </span>
        )}
        {parked && <span data-testid="plan-parked-chip" className="plan-context-chip plan-parked-chip">Parked</span>}
        <span data-testid="plan-context-compact-title" className="plan-context-compact-title">
          {summary.title}
        </span>
        <span data-testid="plan-context-compact-progress" className="plan-context-compact-progress">
          {parked ? "parked" : `${summary.done}/${summary.total}`}
        </span>
      </button>
      {expanded && (
        <div
          id="plan-context-expanded-panel"
          data-testid="plan-context-expanded"
          className="plan-context-expanded"
        >
          <PlanContextBody {...props} />
        </div>
      )}
    </header>
  );
}

export function PlanContextWideHeader(props: PlanContextHeaderProps) {
  const { summary, planId, plan } = props;
  const { collapsed, toggle } = usePlanHeaderCollapse("wide", false, planId);
  const expanded = !collapsed;
  const stateLabel = planningStateLabel(summary.planningState);
  const parked = isPlanParked(plan);

  if (!expanded) {
    return (
      <header
        data-testid="plan-context-header"
        className="plan-context-header plan-context-header-wide is-collapsed"
      >
        <button
          type="button"
          data-testid="plan-context-wide-bar"
          className="plan-context-compact-bar"
          aria-expanded={false}
          aria-controls="plan-context-wide-panel"
          onClick={toggle}
          title="Expand plan context"
        >
          <ChevronDown
            size={16}
            aria-hidden={false}
            aria-label="Expand plan context"
            className="plan-context-compact-chevron"
          />
          {stateLabel && (
            <span
              data-testid="plan-state-chip"
              className={`plan-context-chip plan-state-chip plan-state-${summary.planningState}`}
            >
              {stateLabel}
            </span>
          )}
          {parked && <span data-testid="plan-parked-chip" className="plan-context-chip plan-parked-chip">Parked</span>}
          <span data-testid="plan-context-compact-title" className="plan-context-compact-title">
            {summary.title}
          </span>
          <span data-testid="plan-context-compact-progress" className="plan-context-compact-progress">
            {parked ? "parked" : `${summary.done}/${summary.total}`}
          </span>
        </button>
      </header>
    );
  }

  return (
    <header
      data-testid="plan-context-header"
      className="plan-context-header plan-context-header-wide is-expanded"
    >
      <button
        type="button"
        data-testid="plan-context-wide-chevron"
        className="plan-context-wide-chevron"
        aria-expanded
        aria-controls="plan-context-wide-panel"
        aria-label="Collapse plan context"
        onClick={toggle}
        title="Collapse plan context"
      >
        <ChevronDown size={16} className="plan-context-compact-chevron" />
      </button>
      <div id="plan-context-wide-panel" data-testid="plan-context-wide-panel">
        <PlanContextBody {...props} />
      </div>
    </header>
  );
}

function PlanContextBody({
  summary,
  spec,
  onOpenPlan,
  onOpenSpec,
  onOpenTab,
  plan,
}: PlanContextHeaderProps) {
  const goal = summary.goalVector?.trim() ?? "";
  const stateLabel = planningStateLabel(summary.planningState);
  const plannerLabel = summary.plannerLabel ?? "No planner";
  const parked = isPlanParked(plan);

  return (
    <>
      <div className="plan-context-header-chips" data-testid="plan-context-header-chips">
        {parked && (
          <span data-testid="plan-parked-chip" className="plan-context-chip plan-parked-chip" title="Plan is parked; reminders and heartbeat nudges are quiet">
            Parked
          </span>
        )}
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
        <PairAffordance
          layout="chips"
          data-testid="plan-peer-pair"
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
      {(goal || (!parked && summary.focus)) && (
        <div className="plan-context-header-secondary">
          {goal && (
            <p data-testid="plan-board-plan-goal" className="plan-context-header-goal">
              <span className="plan-context-header-goal-text">{goal}</span>
            </p>
          )}
          {!parked && summary.focus && (
            <p data-testid="plan-board-plan-focus" className="plan-context-header-focus">
              {summary.focus}
            </p>
          )}
        </div>
      )}
      <div className="plan-context-header-main" data-testid="plan-context-header-main">
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
            {summary.ownerLabel} · {parked ? "parked" : progressLabel(summary.done, summary.total)}
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
      </div>
    </>
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
        data-testid={`plan-peer-chip-${role}`}
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
      data-testid={`plan-peer-chip-${role}`}
      className={`plan-context-chip plan-peer-chip plan-peer-chip-${role} plan-peer-chip-static`}
    >
      {content}
    </span>
  );
}
