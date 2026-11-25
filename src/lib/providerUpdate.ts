import { selectedDaemonTransport } from "./daemonRuntime";
import { resolveProviderCliTarget } from "./providerCliTarget";
import { requireProviderInstallLaunch, type ProviderInstallLaunch } from "./providerInstall";
import { relayToPeer } from "./transport/tabs";

export async function launchProviderUpdate(
  providerId: string,
  callerTabId?: string,
  peerId?: string | null,
): Promise<ProviderInstallLaunch> {
  const target = resolveProviderCliTarget({ callerTabId, peerId });
  const path = `/api/agents/providers/${encodeURIComponent(providerId)}/cli`;
  const body = target.callerTabId !== undefined
    ? { operation: "update", caller_tab_id: target.callerTabId }
    : { operation: "update" };
  const result = target.peerId !== undefined
    ? await relayToPeer(
        { json: selectedDaemonTransport.localJson.bind(selectedDaemonTransport) },
        target.peerId,
        "POST",
        path,
        body,
      ) as ProviderInstallLaunch
    : await selectedDaemonTransport.json<ProviderInstallLaunch>("POST", path, body);
  return requireProviderInstallLaunch(result);
}
