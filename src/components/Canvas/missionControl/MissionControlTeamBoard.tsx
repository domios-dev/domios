import { useMemo, useRef } from "react";
import type { NeedsYouDeepLinks } from "../../Notifications/NeedsYouNotificationRow";
import { ModelsCard, NeedsYouPanel, NoActivePlanCard, SpendCard } from "./BoardCards";
import { BoardEventLog } from "./BoardEventLog";
import { boardRefs } from "./BoardRefText";
import { useMeasuredLayout } from "./MissionControlBoard";
import { BOARD_ROW_COLUMNS } from "./missionControlBoardModel";
import type { EventLog, HeldForDecision, MissionControlSpend } from "./missionControlState";
import type { TeamSpend } from "./teamSpend";
import type { TeamMissionControlView } from "./useTeamMissionControl";

const NO_MACHINE_SPEND: MissionControlSpend = {
  scope: "machine",
  state: "not_recording",
  not_recording_reason: "no_rows_for_plan",
  enable_setting: null,
  measured_tokens: 0,
  turns: 0,
  providers: [],
};

const NOTHING_HELD: HeldForDecision = {
  held_reviews: [],
  open_gates: [],
  oldest_hold_age_ms: null,
  verdicts_on_held: 0,
};

const NO_PLAN_LOG: EventLog = { recorded_events: 0, truncated: false, entries: [] };

export interface MissionControlTeamBoardProps {
  team: TeamMissionControlView;
  teamSpend?: TeamSpend | null;
  teamWeekly?: TeamSpend | null;
  deepLinks?: NeedsYouDeepLinks;
  onOpenPlan?: (planId: string) => void;
}

/** The team's Board while no plan is active: what the team spent, the plans it ran, what waits, and what happened. */
export function MissionControlTeamBoard({
  team,
  teamSpend = null,
  teamWeekly = null,
  deepLinks,
  onOpenPlan,
}: MissionControlTeamBoardProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const layout = useMeasuredLayout(rootRef);
  const refs = useMemo(() => boardRefs(team.archivedPlans, undefined, onOpenPlan), [team.archivedPlans, onOpenPlan]);

  return (
    <div
      ref={rootRef}
      data-testid="mission-control-board"
      data-board-layout={layout}
      data-board-scope="team"
      className="mc-board"
    >
      <div
        data-testid="mc-board-row"
        className="mc-board-row"
        style={{ gridTemplateColumns: BOARD_ROW_COLUMNS[layout] }}
      >
        <SpendCard teamSpend={teamSpend} teamWeekly={teamWeekly} />
        <ModelsCard spend={NO_MACHINE_SPEND} teamSpend={teamSpend} />
        <NoActivePlanCard previousPlans={team.archivedPlans} onOpenPlan={onOpenPlan} />
      </div>
      <NeedsYouPanel held={NOTHING_HELD} refs={refs} deepLinks={deepLinks} />
      <BoardEventLog log={NO_PLAN_LOG} milestoneKinds={[]} refs={refs} pager={team.events} />
    </div>
  );
}
