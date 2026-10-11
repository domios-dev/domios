import type { Tab, Team } from "./types";

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

export interface TeamMemberRequest {
  teamId: string;
  role: TeamMemberRole;
  providerId: string;
  managerTabId: string | null;
  model?: string;
  brief?: string;
  sessionBindingNonce?: string;
}

export interface TeamMemberAttachment {
  tab_id: string;
  team: Team;
  reused: boolean;
}

export function selectTeamStaffingManager(tabs: readonly Pick<Tab, "id" | "can_spawn" | "role">[]): string | null {
  return tabs.find((tab) => tab.can_spawn === true && tab.role === "Manager")?.id ?? null;
}

export function requestTeamMember(invoke: Invoke, request: TeamMemberRequest): Promise<unknown> {
  return invoke("mux_add_team_member", {
    teamId: request.teamId,
    role: request.role,
    provider: request.providerId,
    ...(request.model ? { model: request.model } : {}),
    ...(request.brief !== undefined ? { brief: request.brief } : {}),
    callerTabId: request.managerTabId,
    ...(request.sessionBindingNonce ? { sessionBindingNonce: request.sessionBindingNonce } : {}),
  });
}

export function buildTeamActions({
  invoke,
  callerNonce,
}: {
  invoke: Invoke;
  callerNonce?: (tabId: string) => string | undefined;
}): TeamActions {
  return {
    async onRenameTeam(teamId, name, managerTabId) {
      await invoke("mux_rename_team", { teamId, name, callerTabId: managerTabId, ...(managerTabId && callerNonce ? { sessionBindingNonce: callerNonce(managerTabId) } : {}) });
    },
    async onAddTeamMember(teamId, role, providerId, managerTabId, model) {
      await requestTeamMember(invoke, { teamId, role, providerId, managerTabId, model, sessionBindingNonce: managerTabId ? callerNonce?.(managerTabId) : undefined });
    },
    async onCloseTeam(teamId) {
      // The Sidebar confirmation dialog is the human override for busy Workers.
      const result = await invoke("mux_close_team", { teamId, forceBusy: true });
      if (typeof result !== "object" || result === null || !("inventory" in result)) {
        throw new Error("Team close response did not include its lane inventory");
      }
      return (result as { inventory: TeamCloseInventory }).inventory;
    },
  };
}
