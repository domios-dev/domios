import { useState } from "react";
import type { Team } from "../../lib/types";
import { HostAutomationGraph } from "./HostAutomationGraph";
import { AUTOMATION_BUTTON } from "./styles";
import type { AutomationGraphAdapter } from "./types";

export function WorkspaceAutomationView({ workspaceId, teams, onClose, adapter }: { workspaceId: string; teams: readonly Team[]; onClose: () => void; adapter?: AutomationGraphAdapter }) {
  const [teamId, setTeamId] = useState<string | null>(null);
  const scopedTeams = teams.filter((team) => team.workspace_id === workspaceId);
  const teamAvailable = teamId === null || scopedTeams.some((team) => team.id === teamId);
  return <section className="h-full overflow-y-auto bg-[var(--ct-bg)] text-[var(--ct-fg)]" aria-label="Workspace automations">
    <header className="flex flex-wrap items-center justify-between gap-3 border-b border-[var(--ct-border)] p-3">
      <label className="flex items-center gap-2 text-sm">Automation Team
        <select aria-label="Automation Team" className={AUTOMATION_BUTTON} value={teamId ?? ""} onChange={(event) => setTeamId(event.target.value || null)}>
          <option value="">No Team</option>
          {scopedTeams.map((team) => <option key={team.id} value={team.id}>{team.name ?? team.id}</option>)}
          {!teamAvailable && <option value={teamId!}>Unavailable Team</option>}
        </select>
      </label>
      <button type="button" className={AUTOMATION_BUTTON} onClick={onClose}>Close automations</button>
    </header>
    {teamAvailable ? <HostAutomationGraph workspace_id={workspaceId} team_id={teamId} adapter={adapter} /> : <p role="alert" className="p-3">The selected Team is unavailable.</p>}
  </section>;
}
