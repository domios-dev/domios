import { useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import type { Tab } from "../../../lib/types";
import { deriveBoardPlanSummary } from "../planView/boardModel";
import { PlanBoardStatusHeader } from "../planView/PlanBoardStatusHeader";
import type { Plan } from "../types";
import { AreasCard, BouncesCard, HeldCard, RunwayCard, TokensCard, TurnsCard } from "./BoardCards";
import { BoardEventLog } from "./BoardEventLog";
import {
  BOARD_CARD_COLUMNS,
  boardLayout,
  formatCompact,
  formatCount,
  progressReading,
  type BoardLayout,
} from "./missionControlBoardModel";
import { MissionControlControls, type MissionControlPlanAction } from "./MissionControlControls";
import type { PlanBoardState } from "./missionControlState";

export interface MissionControlBoardProps {
  plan: Plan;
  state: PlanBoardState | null;
  /** Set while the first snapshot for this plan is still in flight. */
  loading?: boolean;
  error?: string | null;
  tabs?: Record<string, Tab>;
  now?: number;
  onOpenPlan?: (planId: string) => void;
  onOpenTab?: (tabId: string) => void;
  onApprovePlan?: MissionControlPlanAction;
  onPausePlan?: MissionControlPlanAction;
  onClearEverything?: MissionControlPlanAction;
}

function useMeasuredLayout(ref: RefObject<HTMLDivElement | null>): BoardLayout {
  const [layout, setLayout] = useState<BoardLayout>("wide");
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect?.width ?? 0;
      setLayout(boardLayout(width));
    });
    observer.observe(el);
    setLayout(boardLayout(el.clientWidth));
    return () => observer.disconnect();
  }, [ref]);
  return layout;
}

/** Null until the snapshot lands, so the header keeps its own done/total label. */
function progressLine(state: PlanBoardState | null) {
  if (!state) return null;
  const progress = progressReading(state.task_counts);
  const spend = state.mission_control.kpi.spend;
  return (
    <>
      <span data-testid="mc-board-progress">
        {`${formatCount(progress.done)} of ${formatCount(progress.active)} done`}
        {progress.inReview > 0 && ` · ${formatCount(progress.inReview)} in review`}
        {progress.cancelled > 0 && ` · ${formatCount(progress.cancelled)} cancelled`}
      </span>
      {spend && (
        <span data-testid="mc-board-total-spend">
          {" · "}
          <b>{formatCompact(spend.tokens.output)}</b> output tokens ·{" "}
          <b>{formatCompact(spend.tokens.input)}</b> read
        </span>
      )}
    </>
  );
}

export function MissionControlBoard({
  plan,
  state,
  loading = false,
  error = null,
  tabs = {},
  now = Date.now(),
  onOpenPlan,
  onOpenTab,
  onApprovePlan,
  onPausePlan,
  onClearEverything,
}: MissionControlBoardProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const layout = useMeasuredLayout(rootRef);
  const summary = useMemo(() => deriveBoardPlanSummary(plan, tabs), [plan, tabs]);
  const mc = state?.mission_control ?? null;

  return (
    <div
      ref={rootRef}
      data-testid="mission-control-board"
      data-board-layout={layout}
      className="mc-board"
    >
      <div className="mc-board-header">
        <PlanBoardStatusHeader
          summary={summary}
          onOpenPlan={() => onOpenPlan?.(plan.id)}
          onOpenTab={onOpenTab}
          progress={progressLine(state)}
        />
        <MissionControlControls
          scope="board"
          planId={plan.id}
          onApprovePlan={onApprovePlan}
          onPausePlan={onPausePlan}
          onClearEverything={onClearEverything}
        />
      </div>
      {error && (
        <p data-testid="mc-board-error" className="mc-board-note mc-board-note-error">
          {error}
        </p>
      )}
      {!mc && !error && (
        <p data-testid="mc-board-pending" className="mc-board-note">
          {loading ? "Reading the plan record…" : "No stored Mission Control reading for this plan."}
        </p>
      )}
      {mc && (
        <>
          <div
            data-testid="mc-board-card-grid"
            className="mc-board-card-grid"
            style={{ gridTemplateColumns: BOARD_CARD_COLUMNS[layout] }}
          >
            <TokensCard kpi={mc.kpi} />
            <TurnsCard kpi={mc.kpi} />
            <BouncesCard bounces={mc.bounces} />
            <HeldCard held={mc.held} />
          </div>
          <AreasCard areas={mc.areas} />
          <RunwayCard runway={mc.runway} layout={layout} now={now} />
          <BoardEventLog log={mc.event_log} milestoneKinds={plan.milestone_kinds ?? []} />
        </>
      )}
    </div>
  );
}
