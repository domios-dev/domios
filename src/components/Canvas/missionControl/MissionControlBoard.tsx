import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import type { Plan } from "../types";
import type { AllowanceWeeks } from "./allowanceWeek";
import { NeedsYouPanel, ProgressPanel, SpendCard } from "./BoardCards";
import { BoardEventLog } from "./BoardEventLog";
import { BOARD_ROW_COLUMNS, boardLayout, type BoardLayout } from "./missionControlBoardModel";
import type { PlanBoardState } from "./missionControlState";

export interface MissionControlBoardProps {
  plan: Plan;
  state: PlanBoardState | null;
  /** Set while the first snapshot for this plan is still in flight. */
  loading?: boolean;
  error?: string | null;
  now?: number;
  /** Day-by-day weekly allowance per provider, read outside the Board. */
  allowanceWeeks?: AllowanceWeeks;
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
  allowanceWeeks,
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
          <NeedsYouPanel held={mc.held} />
          <div
            data-testid="mc-board-row"
            className="mc-board-row"
            style={{ gridTemplateColumns: BOARD_ROW_COLUMNS[layout] }}
          >
            <ProgressPanel
              counts={boardState.task_counts}
              kpi={mc.kpi}
              bounces={mc.bounces}
              areas={mc.areas}
            />
            <SpendCard spend={mc.spend} weeks={allowanceWeeks} />
          </div>
          <BoardEventLog log={mc.event_log} milestoneKinds={plan.milestone_kinds ?? []} />
        </>
      )}
    </div>
  );
}
