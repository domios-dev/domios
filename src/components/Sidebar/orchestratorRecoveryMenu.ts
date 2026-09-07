import { invoke } from "../../lib/bridge";
import { ORCHESTRATOR_KIND, roleKind } from "../../lib/roleKind";
import type { ProviderDescriptor, Tab } from "../../lib/types";

export interface OrchestratorRecoveryMenuView {
  teamId: string;
  providers: ProviderDescriptor[];
}

export interface OrchestratorRecoveryApi {
  replace: (teamId: string, providerId: string) => Promise<void>;
}

const defaultApi: OrchestratorRecoveryApi = {
  replace: async (teamId, providerId) => {
    await invoke("mux_add_team_member", {
      teamId,
      provider: providerId,
      role: "orchestrator",
    });
  },
};

export function orchestratorRecoveryMenuView(
  tab: Tab,
  descriptors: ProviderDescriptor[],
  orchestrators: Tab[] | undefined,
): OrchestratorRecoveryMenuView | null {
  if (!orchestrators) return null;
  const teamId = tab.team_id?.trim();
  const absentShell = tab.agent_type == null
    && tab.pty_id != null
    && tab.backing?.kind !== "chat_backend"
    && tab.boot_stage === "shell_ready"
    && tab.agent_booting !== true;
  const hasCompetingAnchor = orchestrators.some((candidate) => (
    candidate.id !== tab.id
    && candidate.team_id === teamId
    && roleKind(candidate) === ORCHESTRATOR_KIND
  ));
  if (
    !absentShell
    || roleKind(tab) !== ORCHESTRATOR_KIND
    || !teamId
    || hasCompetingAnchor
  ) return null;
  const providers = descriptors.filter((descriptor) => descriptor.transport === "pty");
  return providers.length > 0 ? { teamId, providers } : null;
}

export async function replaceOrchestrator(
  view: OrchestratorRecoveryMenuView,
  providerId: string,
  api: OrchestratorRecoveryApi = defaultApi,
): Promise<void> {
  if (!view.providers.some((provider) => provider.id === providerId)) {
    throw new Error(`Provider ${providerId} is not eligible for PTY recovery`);
  }
  await api.replace(view.teamId, providerId);
}
