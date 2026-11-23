import "../../../../src/components/Canvas/missionControl/missionControl.css";
import "../../../../src/components/Canvas/missionControl/missionControlBoard.css";
import { useMemo } from "react";
import { MissionControlTeamBoard } from "../../../../src/components/Canvas/missionControl/MissionControlTeamBoard";
import type { TeamMissionControlView } from "../../../../src/components/Canvas/missionControl/useTeamMissionControl";
import { MEDIA_EVENTS, MEDIA_PLAN, TEAM_SPEND } from "./routes";

/** The Media team's Board as the app shows a team with no active plan: what it spent, by model, and the plan it ran last. */
export function UsagePane() {
  const team = useMemo<TeamMissionControlView>(() => ({
    activePlan: null,
    archivedPlans: [MEDIA_PLAN],
    events: { pageCount: 1, page: (index) => (index === 0 ? MEDIA_EVENTS : null), caption: `${MEDIA_EVENTS.length} events` },
  }), []);
  return (
    <div className="h-full w-full overflow-y-auto" style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
      <MissionControlTeamBoard team={team} teamSpend={TEAM_SPEND} />
    </div>
  );
}
