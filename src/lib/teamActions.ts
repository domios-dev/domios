export interface TeamActions {
  onRenameTeam: (teamId: string, name: string, managerTabId: string) => Promise<void>;
  onAddTeamMember: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerTabId: string,
    model?: string,
  ) => Promise<void>;
  onCloseTeam: (teamId: string, managerTabId: string) => Promise<void>;
}

export type TeamMemberRole = "orchestrator" | "planner" | "worker";

type Invoke = (command: string, args: Record<string, unknown>) => Promise<unknown>;

export function buildTeamActions({
  invoke,
}: {
  invoke: Invoke;
}): TeamActions {
  return {
    async onRenameTeam(teamId, name, managerTabId) {
      await invoke("mux_rename_team", { teamId, name, callerTabId: managerTabId });
    },
    async onAddTeamMember(teamId, role, providerId, managerTabId, model) {
      await invoke("mux_add_team_member", {
        teamId,
        role,
        provider: providerId,
        ...(model ? { model } : {}),
        callerTabId: managerTabId,
      });
    },
    async onCloseTeam(teamId, managerTabId) {
      await invoke("mux_close_team", { teamId, callerTabId: managerTabId });
    },
  };
}
