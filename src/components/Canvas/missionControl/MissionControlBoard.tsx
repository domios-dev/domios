import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import type { Plan } from "../types";
import {
  AreasCard,
  BouncesCard,
  DecisionsWaitingCard,
  DoneFirstTryCard,
  HeldCard,
  LongestWaitCard,
  SpendCard,
  TaskCountsCard,
  TokensCard,
  TurnsCard,
} from "./BoardCards";
import { BoardEventLog } from "./BoardEventLog";
import { BOARD_CARD_COLUMNS, boardLayout, type BoardLayout } from "./missionControlBoardModel";
import { MissionControlControls, type MissionControlPlanAction } from "./MissionControlControls";
import type { PlanBoardState } from "./missionControlState";

export interface MissionControlBoardProps {
  plan: Plan;
  state: PlanBoardState | null;
  /** Set while the first snapshot for this plan is still in flight. */
  loading?: boolean;
  error?: string | null;
  now?: number;
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

export function MissionControlBoard({
  plan,
  state,
  loading = false,
  error = null,
  now = Date.now(),
  onApprovePlan,
  onPausePlan,
  onClearEverything,
}: MissionControlBoardProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const layout = useMeasuredLayout(rootRef);
  const boardState = state?.mission_control ? state : null;
  const mc = boardState?.mission_control ?? null;

  return (
    <div
      ref={rootRef}
      data-testid="mission-control-board"
      data-board-layout={layout}
      className="mc-board"
    >
      <div className="mc-board-actions-row" data-testid="mission-control-board-actions-row">
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
      {boardState && mc && (
        <>
          <div
            data-testid="mc-board-card-grid"
            className="mc-board-card-grid"
            style={{ gridTemplateColumns: BOARD_CARD_COLUMNS[layout] }}
          >
            <TaskCountsCard counts={boardState.task_counts} />
            <TokensCard kpi={mc.kpi} />
            <TurnsCard kpi={mc.kpi} />
            <BouncesCard bounces={mc.bounces} />
            <DecisionsWaitingCard held={mc.held} />
            <HeldCard held={mc.held} />
            <LongestWaitCard held={mc.held} />
            <DoneFirstTryCard bounces={mc.bounces} />
          </div>
          <AreasCard areas={mc.areas} />
          <SpendCard spend={mc.spend} layout={layout} now={now} />
          <BoardEventLog log={mc.event_log} milestoneKinds={plan.milestone_kinds ?? []} />
        </>
      )}
    </div>
  );
}
