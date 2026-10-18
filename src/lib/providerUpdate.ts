import { selectedDaemonTransport } from "./daemonRuntime";
import { requireProviderInstallLaunch, type ProviderInstallLaunch } from "./providerInstall";

export async function launchProviderUpdate(providerId: string, callerTabId?: string): Promise<ProviderInstallLaunch> {
  const result = await selectedDaemonTransport.json<ProviderInstallLaunch>("POST", `/api/agents/providers/${encodeURIComponent(providerId)}/cli`, {
    operation: "update", caller_tab_id: callerTabId,
  });
  return requireProviderInstallLaunch(result);
}
