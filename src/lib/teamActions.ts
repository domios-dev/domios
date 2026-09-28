export interface TeamActions {
  onRenameTeam: (teamId: string, name: string, managerTabId: string | null) => Promise<void>;
  onAddTeamMember: (
    teamId: string,
    role: TeamMemberRole,
    providerId: string,
    managerTabId: string | null,
    model?: string,
  ) => Promise<void>;
  onCloseTeam: (teamId: string) => Promise<TeamCloseInventory>;
}

export interface TeamCloseLaneRecord {
  path: string | null;
  branch: string | null;
  reason: string | null;
}

export interface TeamCloseInventory {
  removed: TeamCloseLaneRecord[];
  kept: TeamCloseLaneRecord[];
  orphan_branches: string[];
}

export type TeamMemberRole = "orchestrator" | "planner" | "cartographer" | "worker";

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
    async onCloseTeam(teamId) {
      const result = await invoke("mux_close_team", { teamId });
      if (typeof result !== "object" || result === null || !("inventory" in result)) {
        throw new Error("Team close response did not include its lane inventory");
      }
      return (result as { inventory: TeamCloseInventory }).inventory;
    },
  };
}
