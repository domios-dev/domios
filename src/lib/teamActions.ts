export interface TeamActions {
  onRenameTeam: (teamId: string, name: string, managerPaneId: string) => Promise<void>;
  onAddTeamMember: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerPaneId: string,
    model?: string,
  ) => Promise<void>;
  onCloseTeam: (teamId: string, managerPaneId: string) => Promise<void>;
}

export type TeamMemberRole = "planner" | "worker";

type Invoke = (command: string, args: Record<string, unknown>) => Promise<unknown>;

export function buildTeamActions({
  invoke,
}: {
  invoke: Invoke;
}): TeamActions {
  return {
    async onRenameTeam(teamId, name, managerPaneId) {
      await invoke("mux_rename_team", { teamId, name, callerTabId: managerPaneId });
    },
    async onAddTeamMember(teamId, role, providerId, managerPaneId, model) {
      await invoke("mux_add_team_member", {
        teamId,
        role,
        provider: providerId,
        ...(model ? { model } : {}),
        callerTabId: managerPaneId,
      });
    },
    async onCloseTeam(teamId, managerPaneId) {
      await invoke("mux_close_team", { teamId, callerTabId: managerPaneId });
    },
  };
}
