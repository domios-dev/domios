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
): OrchestratorRecoveryMenuView | null {
  const teamId = tab.team_id?.trim();
  const absentShell = tab.agent_type == null
    && tab.pty_id != null
    && tab.backing?.kind !== "chat_backend"
    && tab.boot_stage === "shell_ready"
    && tab.agent_booting !== true;
  if (!absentShell || roleKind(tab) !== ORCHESTRATOR_KIND || !teamId) return null;
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
