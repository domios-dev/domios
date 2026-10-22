import { BoardCardReading } from "./BoardCardSkeleton";
import { InlineErrorNotice } from "../../shared/ErrorNotice";
import type { ApiConfig } from "../../../lib/types";
import { UnderwayBoardRow } from "./UnderwayPanel";
import { useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import type { Plan } from "../types";
import type { NeedsYouDeepLinks } from "../../Notifications/NeedsYouNotificationRow";
import { ModelsCard, NeedsYouPanel, PlanCard, SpendCard } from "./BoardCards";
import { BoardEventLog } from "./BoardEventLog";
import { ConnectedPlanSettings } from "./ConnectedPlanSettings";
import { boardRefs } from "./BoardRefText";
import { BOARD_ROW_COLUMNS, boardLayout, type BoardLayout, type EventPager } from "./missionControlBoardModel";
import type { TeamPlanSummary, PlanBoardState } from "./missionControlState";
import type { TeamSpend } from "./teamSpend";

export interface MissionControlBoardProps {
  plan: TeamPlanSummary | Plan;
  state: PlanBoardState | null;
  /** Set while the first snapshot for this plan is still in flight. */
  loading?: boolean;
  error?: string | null;
  now?: number;
  /** Plans whose ids and task ids the Board names by title; defaults to this plan. */
  plans?: readonly Plan[];
  /** Archived plans the same team ran before the active one, newest first. */
  previousPlans?: readonly TeamPlanSummary[];
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
  api?: ApiConfig;
  spendLoading?: boolean;
  spendError?: string | null;
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
  spendLoading = false,
  spendError = null,
  api,
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
      <ConnectedPlanSettings key={plan.id} plan={plan} api={api} initial={state?.control_loop ?? plan.control_loop} now={now} />
      {spendError && <InlineErrorNotice role="alert" error={spendError} operation="loading team spend" />}
      {error && (
        <div data-testid="mc-board-error"><InlineErrorNotice error={error} operation="loading plans" /></div>
      )}
      {!mc && !error && (
        <p data-testid="mc-board-pending" className="mc-board-note">
          {loading ? "Reading the plan record…" : "No stored Mission Control reading for this plan."}
        </p>
      )}
      <div data-testid="mc-board-row" className="mc-board-row" style={{ gridTemplateColumns: BOARD_ROW_COLUMNS[layout] }}>
        <BoardCardReading title="Spend" testId="mc-board-spend" ready={!spendLoading || Boolean(teamSpend)} error={spendError}>
          <SpendCard teamSpend={teamSpend} />
        </BoardCardReading>
        <BoardCardReading title="Models" testId="mc-board-models" ready={Boolean(teamSpend) || !spendLoading && Boolean(mc)} error={error || spendError}>
          <ModelsCard spend={mc?.spend} teamSpend={teamSpend} />
        </BoardCardReading>
        <BoardCardReading title="Plan" testId="mc-board-plan" ready={Boolean(boardState && mc)} error={error}>
          {boardState && mc && <PlanCard plan={plan} api={api} counts={boardState.task_counts} bounces={mc.bounces}
            previousPlans={previousPlans} onOpenPlan={onOpenPlan} onOpenPlanView={onOpenView && (() => onOpenView("plan"))} />}
        </BoardCardReading>
      </div>
      <NeedsYouPanel refs={refs} deepLinks={deepLinks} />
      {plan.team_id && <UnderwayBoardRow api={api} teamId={plan.team_id} plans={plans ?? ("tasks" in plan ? [plan] : [])} refs={refs} deepLinks={deepLinks} />}
      <BoardCardReading title="Event log" testId="mc-board-event-log" ready={Boolean(mc || eventPager)} error={error}>
        <BoardEventLog
          log={mc?.event_log ?? { recorded_events: 0, truncated: false, entries: [] }}
          milestoneKinds={("milestone_kinds" in plan ? plan.milestone_kinds : undefined) ?? plans?.find((candidate) => candidate.id === plan.id)?.milestone_kinds ?? []}
          refs={refs}
          pager={eventPager}
          now={now}
        />
      </BoardCardReading>
    </div>
  );
}
