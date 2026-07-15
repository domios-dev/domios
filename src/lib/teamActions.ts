export interface TeamActions {
  onRenameTeam: (teamId: string, name: string) => Promise<void>;
  onAddTeamMember?: undefined;
}

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
  };
}
