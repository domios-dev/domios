export interface TeamActions {
  onRenameTeam: (teamId: string, name: string) => Promise<void>;
  onAddTeamMember: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerPaneId: string,
  ) => Promise<void>;
}

export type TeamMemberRole = "planner" | "worker";

type Invoke = (command: string, args: Record<string, unknown>) => Promise<unknown>;

export function buildTeamActions({
  callerTabId,
  invoke,
}: {
  callerTabId: () => string | null;
  invoke: Invoke;
}): TeamActions {
  return {
    async onRenameTeam(teamId, name) {
      await invoke("mux_rename_team", { teamId, name, callerTabId: callerTabId() });
    },
    async onAddTeamMember(teamId, role, providerId, managerPaneId) {
      await invoke("mux_add_team_member", {
        teamId,
        role,
        provider: providerId,
        callerTabId: managerPaneId,
      });
    },
  };
}
