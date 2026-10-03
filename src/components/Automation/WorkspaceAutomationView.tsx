import { useState } from "react";
import { Workflow, X } from "lucide-react";
import type { Team } from "../../lib/types";
import { JobsView } from "../Jobs/JobsView";
import type { MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import { AUTOMATION_TOOL } from "./styles";
import type { AutomationGraphAdapter } from "./types";

export function WorkspaceAutomationView({ workspaceId, teams, onClose, adapter, initialTeamId = null, initialScenarioId, cronActionAdapter }: {
  workspaceId: string; teams: readonly Team[]; onClose: () => void; adapter?: AutomationGraphAdapter;
  initialTeamId?: string | null; initialScenarioId?: string; cronActionAdapter?: MeshResourceActionAdapter;
}) {
  const [teamId, setTeamId] = useState<string | null>(initialTeamId);
  const scopedTeams = teams.filter((team) => team.workspace_id === workspaceId);
  const teamAvailable = teamId === null || scopedTeams.some((team) => team.id === teamId);
  return <section className="flex h-full flex-col bg-[var(--ct-bg)] text-[var(--ct-fg)]" aria-label="Jobs">
    <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-4 py-3 sm:px-6">
      <Workflow size={18} className="shrink-0 text-[var(--ct-accent)]" aria-hidden="true" />
      <h2 className="text-[length:var(--ct-font-size-md)] font-semibold">Jobs</h2>
      <label className="ml-auto flex min-w-0 items-center gap-2 text-xs text-[var(--ct-muted-fg)]">Team
        <select aria-label="Jobs Team" className="h-8 min-w-0 max-w-[14rem] rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] px-2 text-xs text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]" value={teamId ?? ""} onChange={(event) => setTeamId(event.target.value || null)}>
          <option value="">No Team</option>
          {scopedTeams.map((team) => <option key={team.id} value={team.id}>{team.name ?? team.id}</option>)}
          {!teamAvailable && <option value={teamId!}>Unavailable Team</option>}
        </select>
      </label>
      <button type="button" className={AUTOMATION_TOOL} aria-label="Close jobs" title="Close jobs" onClick={onClose}><X size={15} aria-hidden="true" /></button>
    </header>
    <div className="min-h-0 flex-1 overflow-y-auto">
      {teamAvailable
        ? <JobsView workspaceId={workspaceId} teamId={teamId} adapter={adapter} initialScenarioId={teamId === initialTeamId ? initialScenarioId : undefined} cronActionAdapter={cronActionAdapter} />
        : <p role="alert" className="p-4">The selected Team is unavailable.</p>}
    </div>
  </section>;
}
