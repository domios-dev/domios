import { InlineErrorNotice } from "../../shared/ErrorNotice";
import type { ApiConfig } from "../../../lib/types";
import type { Plan } from "../types";
import { boardRefs } from "./BoardRefText";
import { UnderwayBoardRow } from "./UnderwayPanel";
import { useMemo, useRef } from "react";
import type { NeedsYouDeepLinks } from "../../Notifications/NeedsYouNotificationRow";
import { ModelsCard, NeedsYouPanel, NoActivePlanCard, SpendCard } from "./BoardCards";
import { BoardEventLog } from "./BoardEventLog";
import { BoardCardReading } from "./BoardCardSkeleton";
import { useMeasuredLayout } from "./MissionControlBoard";
import { BOARD_ROW_COLUMNS } from "./missionControlBoardModel";
import type { EventLog } from "./missionControlState";
import type { TeamSpend } from "./teamSpend";
import type { TeamMissionControlView } from "./useTeamMissionControl";

const NO_PLAN_LOG: EventLog = { recorded_events: 0, truncated: false, entries: [] };

export interface MissionControlTeamBoardProps {
  team: TeamMissionControlView | null;
  teamSpend?: TeamSpend | null;
  error?: string | null;
  plans?: readonly Plan[];
  api?: ApiConfig;
  spendLoading?: boolean;
  spendError?: string | null;
  teamId?: string | null;
  deepLinks?: NeedsYouDeepLinks;
  onOpenPlan?: (planId: string) => void;
}

/** The team's Board while no plan is active: what the team spent, the plans it ran, what waits, and what happened. */
export function MissionControlTeamBoard({
  team,
  teamSpend = null,
  error = null,
  plans = [],
  spendLoading = false,
  spendError = null,
  api,
  teamId,
  deepLinks,
  onOpenPlan,
}: MissionControlTeamBoardProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const layout = useMeasuredLayout(rootRef);
  const refs = useMemo(() => boardRefs([...plans, ...(team?.archivedPlans ?? [])], undefined, onOpenPlan), [plans, team?.archivedPlans, onOpenPlan]);

  return (
    <div
      ref={rootRef}
      data-testid="mission-control-board"
      data-board-layout={layout}
      data-board-scope="team"
      className="mc-board"
    >
      {error && <InlineErrorNotice role="alert" error={error} operation="loading team board" />}
      {spendError && <InlineErrorNotice role="alert" error={spendError} operation="loading team spend" />}
      <div
        data-testid="mc-board-row"
        className="mc-board-row"
        style={{ gridTemplateColumns: BOARD_ROW_COLUMNS[layout] }}
      >
        <BoardCardReading title="Spend" testId="mc-board-spend" ready={!spendLoading || Boolean(teamSpend)} error={spendError}>
          <SpendCard teamSpend={teamSpend} />
        </BoardCardReading>
        <BoardCardReading title="Models" testId="mc-board-models" ready={!spendLoading || Boolean(teamSpend)} error={spendError}>
          <ModelsCard teamSpend={teamSpend} />
        </BoardCardReading>
        <BoardCardReading title="Plan" testId="mc-board-plan" ready={Boolean(team)} error={error}>
          {team && <NoActivePlanCard previousPlans={team.archivedPlans} onOpenPlan={onOpenPlan} />}
        </BoardCardReading>
      </div>
      <NeedsYouPanel refs={refs} deepLinks={deepLinks} />
      {teamId && <UnderwayBoardRow api={api} teamId={teamId} plans={plans} refs={refs} deepLinks={deepLinks} />}
      <BoardCardReading title="Event log" testId="mc-board-event-log" ready={Boolean(team)} error={error}>
        {team && <BoardEventLog log={NO_PLAN_LOG} milestoneKinds={[]} refs={refs} pager={team.events} />}
      </BoardCardReading>
    </div>
  );
}
