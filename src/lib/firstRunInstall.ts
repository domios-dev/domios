import { peerEventRail } from "./peerEventRail";
import { localJson } from "./webApi";
import { relayToPeer } from "./transport/tabs";
import { listen } from "./bridge";
import { launchProviderInstaller } from "./providerInstall";

export interface InstallCompletion {
  provider: string;
  operation: string;
  tab_id: string;
  success: boolean;
  summary: string;
}

async function supportsInstallCompletion(peerId: string): Promise<boolean> {
  const identity = await relayToPeer({ json: localJson }, peerId, "GET", "/api/identify", null) as { features?: unknown } | null;
  return Array.isArray(identity?.features) && identity.features.includes("provider_cli_completion_rail");
}

export async function installAndWait(providerId: string, signal: AbortSignal, peerId?: string): Promise<"installed" | "dispatched"> {
  if (peerId !== undefined && !await supportsInstallCompletion(peerId)) {
    if (signal.aborted) throw new Error("Installation canceled");
    await launchProviderInstaller(providerId, peerId);
    return "dispatched";
  }
  const observed = new Map<string, InstallCompletion>();
  let target: string | undefined;
  let accept: (event: InstallCompletion) => void = () => {};
  const completed = new Promise<InstallCompletion>((resolve) => { accept = resolve; });
  const receive = (payload: InstallCompletion) => {
    if (payload.provider !== providerId || payload.operation !== "install") return;
    observed.set(payload.tab_id, payload);
    if (payload.tab_id === target) accept(payload);
  };
  const stop = peerId === undefined
    ? await listen<InstallCompletion>("provider-cli-completed", ({ payload }) => receive(payload))
    : peerEventRail.observe(peerId, event => {
      if (event.type !== "provider-cli-completed") return;
      const { provider, operation, tab_id, success, summary } = event.data;
      if (operation === "install" && tab_id && typeof success === "boolean" && typeof summary === "string") {
        receive({ provider, operation, tab_id, success, summary });
      }
    });
  const fail = (summary: string) => accept({ provider: providerId, operation: "install", tab_id: target ?? "", success: false, summary });
  const abort = () => fail("Installation canceled");
  signal.addEventListener("abort", abort, { once: true });
  const deadline = setTimeout(() => fail("Installation timed out. Retry to install again."), 660_000);
  try {
    if (signal.aborted) throw new Error("Installation canceled");
    const launch = await launchProviderInstaller(providerId, peerId);
    target = launch.tab_id;
    const early = observed.get(target);
    const result = early ?? await completed;
    if (!result.success) throw new Error(result.summary);
    return "installed";
  } finally {
    stop();
    clearTimeout(deadline);
    signal.removeEventListener("abort", abort);
  }
}
