import { useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import type { Plan } from "../types";
import type { NeedsYouDeepLinks } from "../../Notifications/NeedsYouNotificationRow";
import { ModelsCard, NeedsYouPanel, PlanCard, SpendCard } from "./BoardCards";
import { BoardEventLog } from "./BoardEventLog";
import { boardRefs } from "./BoardRefText";
import { BOARD_ROW_COLUMNS, boardLayout, type BoardLayout, type EventPager } from "./missionControlBoardModel";
import type { PlanBoardState } from "./missionControlState";
import type { TeamSpend } from "./teamSpend";

export interface MissionControlBoardProps {
  plan: Plan;
  state: PlanBoardState | null;
  /** Set while the first snapshot for this plan is still in flight. */
  loading?: boolean;
  error?: string | null;
  now?: number;
  /** Plans whose ids and task ids the Board names by title; defaults to this plan. */
  plans?: readonly Plan[];
  /** Archived plans the same team ran before the active one, newest first. */
  previousPlans?: readonly Plan[];
  /** Switches the host to the Spec, Plan or Tasks view. */
  onOpenView?: (mode: "spec" | "plan" | "tasks") => void;
  onOpenTask?: (taskId: string) => void;
  onOpenPlan?: (planId: string) => void;
  /** Needs you notifications in this Board's scope, each one jump to its chat message. */
  deepLinks?: NeedsYouDeepLinks;
  /** The team's event stream; the plan's stored log is used without it. */
  eventPager?: EventPager;
  /** The team's token spend by plan, model and day; machine spend is shown without it. */
  teamSpend?: TeamSpend | null;
  /** The team's weekly allowance points by day, read per provider account. */
  teamWeekly?: TeamSpend | null;
}

export function useMeasuredLayout(ref: RefObject<HTMLDivElement | null>): BoardLayout {
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
  now,
  plans,
  previousPlans,
  onOpenView,
  onOpenTask,
  onOpenPlan,
  deepLinks,
  eventPager,
  teamSpend = null,
  teamWeekly = null,
}: MissionControlBoardProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const layout = useMeasuredLayout(rootRef);
  const refs = useMemo(() => boardRefs(plans ?? [plan], onOpenTask, onOpenPlan), [plans, plan, onOpenTask, onOpenPlan]);
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
          <div
            data-testid="mc-board-row"
            className="mc-board-row"
            style={{ gridTemplateColumns: BOARD_ROW_COLUMNS[layout] }}
          >
            <SpendCard teamSpend={teamSpend} teamWeekly={teamWeekly} />
            <ModelsCard spend={mc.spend} teamSpend={teamSpend} />
            <PlanCard
              plan={plan}
              counts={boardState.task_counts}
              bounces={mc.bounces}
              previousPlans={previousPlans}
              onOpenPlan={onOpenPlan}
              onOpenPlanView={onOpenView && (() => onOpenView("plan"))}
              onOpenTasksView={onOpenView && (() => onOpenView("tasks"))}
            />
          </div>
          <NeedsYouPanel held={mc.held} refs={refs} deepLinks={deepLinks} />
          <BoardEventLog
            log={mc.event_log}
            milestoneKinds={plan.milestone_kinds ?? []}
            refs={refs}
            pager={eventPager}
            now={now}
          />
        </>
      )}
    </div>
  );
}
